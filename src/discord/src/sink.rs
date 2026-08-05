//! Discord Rich Presence sink. Owns one thread and one IPC connection.
//!
//! Threading mirrors `src/mpris/src/sink.rs`: the event-sink closure registered
//! with the playback coordinator only clones and forwards, because it runs on
//! the coordinator worker thread and must never block. All IPC happens here.
//!
//! Note this deliberately does NOT use `jfn_playback::sink_core::run_sink` —
//! that harness is a process-wide singleton already claimed by the SMTC sink on
//! Windows and the Now Playing sink on macOS, so a second caller silently
//! no-ops.

use discord_rich_presence::activity::{
    Activity as IpcActivity, ActivityType, Assets, StatusDisplayType, Timestamps,
};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use jfn_playback::{MediaMetadata, PlaybackEvent, PlaybackEventKind, PlaybackSnapshot};
use parking_lot::Mutex;
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use crate::projection::{self, Activity, Input, Kind, StatusLine};

/// Discord application id — the "Tchê Flix" app in the developer portal. Public
/// by design: it travels in every RPC payload and carries no secret.
const APP_ID: &str = "1012863866331484240";

/// Art asset key uploaded in the portal. Shown when the server isn't publicly
/// reachable and a poster URL would only 404 for Discord's CDN.
const FALLBACK_IMAGE: &str = "tcheflix";

/// Badge overlaid on the poster while paused, and its tooltip. An external URL
/// rather than a portal asset key: an uploaded `paused` asset was never drawn,
/// while this — the same image jellyfin-rpc uses — renders. It is the only
/// paused marker, since the text carries none.
const PAUSED_IMAGE: &str = "https://i.imgur.com/wlHSvYy.png";
const PAUSED_TEXT: &str = "Pausado";

/// Discord rate-limits SET_ACTIVITY to 5 requests per 20s. 4s keeps us inside
/// that with no token bookkeeping while still feeling immediate on pause/seek.
const MIN_UPDATE_INTERVAL: Duration = Duration::from_secs(4);

const RECONNECT_MIN: Duration = Duration::from_secs(5);
const RECONNECT_MAX: Duration = Duration::from_secs(60);

/// Idle tick. Bounds how long a throttled update can sit unsent and how often
/// we retry a connection while Discord isn't running.
const IDLE_TICK: Duration = Duration::from_secs(1);

enum Msg {
    Event(Box<PlaybackEvent>),
    Stop,
}

struct Sink {
    tx: Sender<Msg>,
    join: Option<JoinHandle<()>>,
}

fn sink_slot() -> &'static Mutex<Option<Sink>> {
    static SLOT: OnceLock<Mutex<Option<Sink>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Accumulated state. `content` survives across events (it is pushed from the
/// JS UI); `snapshot` is mpv's authoritative view, refreshed on every event.
#[derive(Default)]
struct State {
    content: MediaMetadata,
    snapshot: PlaybackSnapshot,
    view: Activity,
}

/// Push a `PlaybackEvent` into the running sink. No-op if not started.
/// Called by the event-sink closure registered in [`start`].
pub(crate) fn deliver(ev: PlaybackEvent) {
    if let Some(s) = sink_slot().lock().as_ref() {
        let _ = s.tx.send(Msg::Event(Box::new(ev)));
    }
}

pub(crate) fn start(server_url: fn() -> String, status_title: fn() -> bool) {
    let mut slot = sink_slot().lock();
    if slot.is_some() {
        return;
    }
    let (tx, rx) = channel::<Msg>();
    let join = match std::thread::Builder::new()
        .name("discord-sink".into())
        .spawn(move || worker(rx, server_url, status_title))
    {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!(target: "Discord", "sink thread spawn failed: {e}");
            return;
        }
    };
    *slot = Some(Sink {
        tx,
        join: Some(join),
    });
    // Drop the slot lock before touching the coordinator: register_event_sink
    // reaches into it, and holding both is an ordering hazard.
    drop(slot);

    // Once, not a resettable flag: the coordinator has no unregister API, so a
    // second start() after stop() must not double-register.
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| {
        jfn_playback::register_event_sink(Box::new(|ev: &PlaybackEvent| {
            deliver(ev.clone());
        }));
    });
}

pub(crate) fn stop() {
    let Some(mut s) = sink_slot().lock().take() else {
        return;
    };
    let _ = s.tx.send(Msg::Stop);
    // Deliberately NOT joining, unlike the MPRIS sink. discord-rich-presence
    // does blocking pipe I/O with no timeout, so a wedged Discord client would
    // stall app shutdown indefinitely. The thread observes Stop and exits on
    // its own; the process is going down anyway.
    drop(s.join.take());
}

fn worker(rx: Receiver<Msg>, server_url: fn() -> String, status_title: fn() -> bool) {
    let mut client = DiscordIpcClient::new(APP_ID);
    let mut state = State::default();
    let mut connected = false;
    let mut backoff = RECONNECT_MIN;
    let mut next_connect = Instant::now();
    let mut pending = false;
    let mut last_sent: Option<Instant> = None;

    loop {
        match rx.recv_timeout(IDLE_TICK) {
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(Msg::Event(ev)) => {
                if handle_event(&ev, &mut state, server_url, status_title) {
                    pending = true;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
        }

        if !connected && Instant::now() >= next_connect {
            if client.connect().is_ok() {
                tracing::debug!(target: "Discord", "connected to Discord IPC");
                connected = true;
                backoff = RECONNECT_MIN;
                // Discord starts with nothing, so re-push whatever we have.
                pending = state.view != Activity::default();
                last_sent = None;
            } else {
                // Discord simply isn't running. Expected, not an error.
                next_connect = Instant::now() + backoff;
                backoff = (backoff * 2).min(RECONNECT_MAX);
            }
        }

        let due = last_sent.is_none_or(|t| t.elapsed() >= MIN_UPDATE_INTERVAL);
        if connected && pending && due {
            if push(&mut client, &state.view).is_ok() {
                pending = false;
                last_sent = Some(Instant::now());
            } else {
                // Broken pipe: Discord quit mid-session. Drop to reconnect.
                connected = false;
                let _ = client.close();
                next_connect = Instant::now() + backoff;
                backoff = (backoff * 2).min(RECONNECT_MAX);
            }
        }
    }

    if connected {
        let _ = client.clear_activity();
        let _ = client.close();
    }
}

/// Fold one event into `state`. Returns true when the projected view actually
/// changed and an IPC push is warranted.
fn handle_event(
    ev: &PlaybackEvent,
    state: &mut State,
    server_url: fn() -> String,
    status_title: fn() -> bool,
) -> bool {
    // Unconditional: every event carries the post-batch snapshot, so this keeps
    // position/rate fresh even for the events we otherwise ignore.
    state.snapshot = ev.snapshot.clone();

    let recompute = match ev.kind {
        PlaybackEventKind::MetadataChanged => {
            // Same-Id dedup, as in the MPRIS sink: a repeat setMetadata for the
            // same item is a semantic no-op, and letting it through would
            // clobber cached state on every variant switch.
            let changed = ev.metadata.id.is_empty() || ev.metadata.id != state.content.id;
            if changed {
                state.content = ev.metadata.clone();
            }
            changed
        }
        PlaybackEventKind::Started
        | PlaybackEventKind::Paused
        | PlaybackEventKind::Finished
        | PlaybackEventKind::Canceled
        | PlaybackEventKind::Error
        | PlaybackEventKind::TrackLoaded
        | PlaybackEventKind::DurationChanged
        | PlaybackEventKind::RateChanged
        | PlaybackEventKind::MediaTypeChanged
        | PlaybackEventKind::Seeked => true,

        // Position is already folded into the snapshot above; recomputing on
        // every tick would burn the rate limit for nothing.
        PlaybackEventKind::PositionChanged
        | PlaybackEventKind::SeekingChanged
        | PlaybackEventKind::BufferingChanged
        | PlaybackEventKind::ArtworkChanged
        | PlaybackEventKind::FullscreenChanged
        | PlaybackEventKind::BufferedRangesChanged
        | PlaybackEventKind::DisplayHzChanged
        | PlaybackEventKind::QueueCapsChanged => false,
    };

    if !recompute {
        return false;
    }

    let base = server_url();
    let next = projection::project(&Input {
        phase: state.snapshot.phase,
        media_type: state.content.media_type,
        title: state.content.title.clone(),
        series: state.content.artist.clone(),
        season: state.content.album.clone(),
        episode: state.content.track_number,
        item_id: state.content.id.clone(),
        duration_us: state.content.duration_us,
        rating: state.content.community_rating,
        year: state.content.production_year,
        position_us: state.snapshot.position_us,
        rate: state.snapshot.rate,
        server_url: base.clone(),
        status_title: status_title(),
    });

    if next == state.view {
        return false;
    }
    // Name the reason there's no cover, otherwise the fallback asset key in the
    // activity log looks like a Discord problem when it's really a local address.
    if next.large_image.is_empty() && !next.details.is_empty() {
        tracing::debug!(
            target: "Discord",
            "no cover art: base={base:?} is empty or not reachable from the public \
             internet — set the Discord cover art URL in Client Settings; \
             falling back to the {FALLBACK_IMAGE} asset",
        );
    }
    state.view = next;
    true
}

fn push(client: &mut DiscordIpcClient, view: &Activity) -> Result<(), ()> {
    if view.details.is_empty() && view.state.is_empty() {
        tracing::debug!(target: "Discord", "clearing activity");
        return client.clear_activity().map_err(|_| ());
    }

    // set_activity is fire-and-forget: the crate never reads Discord's reply,
    // so a payload Discord rejects still returns Ok. This log is the only
    // visibility into what we actually sent — notably whether the cover URL is
    // the public override, the connect-time server address, or the fallback
    // asset key.
    tracing::debug!(
        target: "Discord",
        "activity details={:?} state={:?} image={:?} elapsed_s={:?} remaining_s={:?} \
         paused={} status_line={:?}",
        view.details,
        view.state,
        if view.large_image.is_empty() { FALLBACK_IMAGE } else { view.large_image.as_str() },
        view.progress.map(|p| p.elapsed_us / 1_000_000),
        view.progress.map(|p| p.remaining_us / 1_000_000),
        view.paused,
        view.status_line,
    );

    let image = if view.large_image.is_empty() {
        FALLBACK_IMAGE
    } else {
        view.large_image.as_str()
    };
    let mut assets = Assets::new().large_image(image);
    if !view.large_text.is_empty() {
        assets = assets.large_text(view.large_text.as_str());
    }
    if view.paused {
        assets = assets.small_image(PAUSED_IMAGE).small_text(PAUSED_TEXT);
    }

    let activity_type = match view.kind {
        Kind::Music => ActivityType::Listening,
        Kind::Episode | Kind::Movie => ActivityType::Watching,
    };

    // Which field ends up after "Assistindo" in the member list. Unset, Discord
    // defaults to the application name — so `Name` is sent explicitly rather
    // than omitted, otherwise turning the setting off would leave a connected
    // session stuck on the last value Discord saw.
    let status_display_type = match view.status_line {
        StatusLine::Title => StatusDisplayType::Details,
        StatusLine::AppName => StatusDisplayType::Name,
    };

    let mut activity = IpcActivity::new()
        .activity_type(activity_type)
        .status_display_type(status_display_type)
        .details(view.details.as_str())
        .assets(assets);
    if !view.state.is_empty() {
        activity = activity.state(view.state.as_str());
    }

    // Both edges: `end` alone only buys a "45:00 left" counter, while start+end
    // is what makes Discord draw the filled bar. Turned absolute at send time,
    // so a throttled update never ships a position that was already stale when
    // it was queued.
    if let Some(p) = view.progress
        && let Ok(now) = SystemTime::UNIX_EPOCH.elapsed()
    {
        let now_s = i64::try_from(now.as_secs()).unwrap_or(i64::MAX);
        activity = activity.timestamps(
            Timestamps::new()
                .start(now_s.saturating_sub(p.elapsed_us / 1_000_000))
                .end(now_s.saturating_add(p.remaining_us / 1_000_000)),
        );
    }

    client.set_activity(activity).map_err(|_| ())
}
