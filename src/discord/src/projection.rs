//! Pure projection: playback state + Jellyfin metadata -> the Discord activity
//! fields. No IPC, no clock, no globals — everything the user ends up seeing is
//! decided here so it can be unit-tested, mirroring `src/mpris/src/projection.rs`.

use jfn_playback::{MediaType, PlaybackPhase};
use std::net::{Ipv4Addr, Ipv6Addr};

/// Jellyfin's precise `Type` never reaches Rust: `parse_metadata_json` collapses
/// it to Audio/Video. Episode-vs-movie is inferred from the series name, which
/// only episodes carry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Kind {
    Episode,
    #[default]
    Movie,
    Music,
}

/// Wall-clock offsets relative to "now", in microseconds. Kept as offsets
/// rather than absolute timestamps so the projection stays clock-free and
/// diffable; the sink turns them into `Timestamps::start`/`end` at send time.
///
/// Both edges are needed: `end` alone only gets a "45:00 left" counter out of
/// Discord, while `start` + `end` is what renders the filled progress bar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Progress {
    pub elapsed_us: i64,
    pub remaining_us: i64,
}

/// Which field Discord reads for the one-line status in the member list. The
/// activity card always shows everything; this only picks what follows
/// "Assistindo".
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatusLine {
    /// The application name: "Assistindo Tchê Flix".
    #[default]
    AppName,
    /// `details`: "Assistindo Fear The Walking Dead".
    Title,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Input {
    pub phase: PlaybackPhase,
    pub media_type: MediaType,
    /// `MediaMetadata.title` — Jellyfin `Name`.
    pub title: String,
    /// `MediaMetadata.artist` — `SeriesName`, or `Artists[0]` for music.
    pub series: String,
    /// `MediaMetadata.album` — `SeasonName`, or `Album` for music.
    pub season: String,
    /// `MediaMetadata.track_number` — Jellyfin `IndexNumber`.
    pub episode: i32,
    pub item_id: String,
    /// Metadata duration, NOT snapshot duration: the two diverge during track
    /// transitions (see `src/mpris/src/projection.rs`).
    pub duration_us: i64,
    pub position_us: i64,
    pub rate: f64,
    /// `MediaMetadata.community_rating` — Jellyfin `CommunityRating`.
    pub rating: f64,
    /// `MediaMetadata.production_year` — Jellyfin `ProductionYear`.
    pub year: i32,
    /// Server base URL, or empty when unknown.
    pub server_url: String,
    /// Setting: put the item name in the status line instead of the app name.
    /// Read live from the config on every update, like `server_url`.
    pub status_title: bool,
}

/// What the sink pushes to Discord. Diffed field-by-field; only a change here
/// costs an IPC round-trip.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Activity {
    pub kind: Kind,
    /// Line 1 under the app name.
    pub details: String,
    /// Line 2.
    pub state: String,
    /// An https URL, or empty -> the sink substitutes the uploaded asset key.
    pub large_image: String,
    /// Tooltip on the poster.
    pub large_text: String,
    /// `None` -> no progress bar (paused, unknown duration, or already past the
    /// end).
    pub progress: Option<Progress>,
    /// Playback is paused. Discord has no paused state, so the sink hangs a
    /// badge off the poster — the only marker there is.
    pub paused: bool,
    /// Which field Discord shows in the member-list status line.
    pub status_line: StatusLine,
}

/// Discord rejects the whole SET_ACTIVITY payload when `details`, `state` or
/// `large_text` fall outside 3..=128 characters. Since the crate never reads
/// Discord's reply, a violation would silently show nothing at all — so clamp
/// here instead. Padding uses U+200E (left-to-right mark): invisible, and what
/// the reference implementation uses.
fn fit(s: String) -> String {
    let n = s.chars().count();
    if n > DISCORD_FIELD_MAX {
        return s.chars().take(DISCORD_FIELD_MAX).collect();
    }
    if n > 0 && n < DISCORD_FIELD_MIN {
        let mut padded = s;
        for _ in n..DISCORD_FIELD_MIN {
            padded.push('\u{200e}');
        }
        return padded;
    }
    // Empty stays empty: the sink reads that as "clear the presence".
    s
}

const DISCORD_FIELD_MIN: usize = 3;
const DISCORD_FIELD_MAX: usize = 128;

fn kind_of(media_type: MediaType, series: &str) -> Kind {
    match media_type {
        MediaType::Audio => Kind::Music,
        MediaType::Video | MediaType::Unknown => {
            if series.is_empty() {
                Kind::Movie
            } else {
                Kind::Episode
            }
        }
    }
}

/// `"Season 1"` / `"Temporada 1"` -> `Some(1)`. Jellyfin sends the season as a
/// localized display string; only the digits are portable across languages.
fn season_number(season: &str) -> Option<u32> {
    let digits: String = season
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// `T1:E5`, `T1`, `E5`, or empty when neither is known.
fn season_episode_label(season: &str, episode: i32) -> String {
    match (season_number(season), episode) {
        (Some(s), e) if e > 0 => format!("T{s}:E{e}"),
        (Some(s), _) => format!("T{s}"),
        (None, e) if e > 0 => format!("E{e}"),
        (None, _) => String::new(),
    }
}

/// `⭐ 8.4 · 2024`, either half alone, or empty when neither is known. This is
/// what a movie gets on line 2, where an episode gets `T1:E5 · Title`.
fn rating_year_label(rating: f64, year: i32) -> String {
    let rating = (rating.is_finite() && rating > 0.0).then(|| format!("⭐ {rating:.1}"));
    let year = (year > 0).then(|| year.to_string());
    match (rating, year) {
        (Some(r), Some(y)) => format!("{r} · {y}"),
        (Some(r), None) => r,
        (None, Some(y)) => y,
        (None, None) => String::new(),
    }
}

/// Both edges of the bar as wall-clock durations, scaled by playback rate.
/// `None` whenever a bar would be wrong or meaningless.
fn progress(input: &Input) -> Option<Progress> {
    // Discord has no paused state: the client animates the bar locally, so one
    // left up while paused just drifts. Drop it instead — it comes back
    // recomputed on resume.
    if input.phase != PlaybackPhase::Playing {
        return None;
    }
    if input.duration_us <= 0 {
        return None;
    }
    let left = input.duration_us.saturating_sub(input.position_us);
    if left <= 0 {
        return None;
    }
    // rate <= 0 means stalled or reversed — there is no honest finish time.
    if !input.rate.is_finite() || input.rate <= 0.0 {
        return None;
    }
    // Scaling both edges keeps the filled fraction honest and lands the bar on
    // the end at the right wall-clock moment. The trade-off is that the total
    // Discord prints is time-to-finish, not the media runtime — identical at
    // 1.0x, which is every ordinary playback.
    let remaining = left as f64 / input.rate;
    let elapsed = input.position_us.max(0) as f64 / input.rate;
    if !remaining.is_finite() || remaining < 1.0 || !elapsed.is_finite() {
        return None;
    }
    Some(Progress {
        elapsed_us: elapsed as i64,
        remaining_us: remaining as i64,
    })
}

/// Host portion of a URL: no scheme, userinfo, port, or path. IPv6 literals
/// keep their brackets.
fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if let Some(end) = authority.find(']') {
        // IPv6 literal: keep through the closing bracket, drop any :port.
        return authority.get(..=end).unwrap_or(authority);
    }
    authority.split_once(':').map_or(authority, |(h, _)| h)
}

/// Conservative: loopback, RFC1918, CGNAT, link-local and bare hostnames are all
/// treated as unreachable, because Discord's CDN fetches the poster from the
/// public internet and would only render a broken box.
fn is_publicly_reachable(url: &str) -> bool {
    let host = host_of(url).to_ascii_lowercase();
    if host.is_empty()
        || host == "localhost"
        || host.ends_with(".local")
        || host.ends_with(".localhost")
        || host.ends_with(".internal")
    {
        return false;
    }
    if let Ok(v4) = host.parse::<Ipv4Addr>() {
        // 100.64.0.0/10 (CGNAT) has no stable std predicate.
        let cgnat = v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1]);
        return !(cgnat
            || v4.is_private()
            || v4.is_loopback()
            || v4.is_link_local()
            || v4.is_unspecified()
            || v4.is_broadcast());
    }
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(v6) = bare.parse::<Ipv6Addr>() {
        // fc00::/7 (unique local) and fe80::/10 (link local) have no stable
        // std predicates either.
        let o = v6.octets();
        let ula = o[0] & 0xfe == 0xfc;
        let link_local = o[0] == 0xfe && o[1] & 0xc0 == 0x80;
        return !(ula || link_local || v6.is_loopback() || v6.is_unspecified());
    }
    // A dotless name is an intranet host, not a public FQDN.
    host.contains('.')
}

fn image_url(input: &Input) -> String {
    if input.item_id.is_empty() {
        return String::new();
    }
    let base = input.server_url.trim().trim_end_matches('/');
    if base.is_empty() || !is_publicly_reachable(base) {
        return String::new();
    }
    let id = &input.item_id;
    // No query string: Discord proxies this URL through its own CDN and resizes
    // it there, and the reference implementation (jellyfin-rpc) passes the bare
    // path. Adding `?maxWidth=` buys nothing and is one more thing the proxy
    // can choke on.
    //
    // The override is a free-text field, so a bare host is fair game.
    if base.contains("://") {
        format!("{base}/Items/{id}/Images/Primary")
    } else {
        format!("https://{base}/Items/{id}/Images/Primary")
    }
}

pub fn project(input: &Input) -> Activity {
    // Nothing is playing. Emit an empty activity so the sink clears the
    // presence outright: an idle client should show no "Watching Tchê Flix"
    // at all. Metadata deliberately isn't consulted here — it outlives
    // playback (the coordinator never clears it), so gating on phase is what
    // stops a finished episode from lingering in the status.
    if input.phase == PlaybackPhase::Stopped {
        return Activity::default();
    }

    let kind = kind_of(input.media_type, &input.series);
    let paused = input.phase == PlaybackPhase::Paused;

    // "Série em destaque": the container goes on line 1, the specific item on
    // line 2. Music follows Discord's own convention (track first) instead,
    // matching what users already see from Spotify.
    let (details, mut state) = match kind {
        Kind::Episode => {
            let label = season_episode_label(&input.season, input.episode);
            let state = match (label.as_str(), input.title.as_str()) {
                ("", t) => t.to_string(),
                (l, "") => l.to_string(),
                (l, t) => format!("{l} · {t}"),
            };
            (input.series.clone(), state)
        }
        // A movie has no episode to name, so line 2 goes to the score and year.
        Kind::Movie => (
            input.title.clone(),
            rating_year_label(input.rating, input.year),
        ),
        Kind::Music => {
            let state = match (input.series.as_str(), input.season.as_str()) {
                ("", "") => String::new(),
                (a, "") => a.to_string(),
                ("", b) => b.to_string(),
                (a, b) => format!("{a} · {b}"),
            };
            (input.title.clone(), state)
        }
    };

    // Line 2 leads with a glyph for the kind of content. Pure decoration — the
    // pause marker is the badge the sink hangs off the poster, and the status
    // line reads `details`, so nothing here carries information. Movies are
    // left alone: their line already opens with the ⭐.
    let icon = match kind {
        Kind::Episode => Some("🎬"),
        Kind::Music => Some("🎵"),
        Kind::Movie => None,
    };
    if let Some(icon) = icon
        && !state.is_empty()
    {
        state = format!("{icon} {state}");
    }

    // Never empty: an unset large_text on a set large_image renders badly, and
    // the reference implementation always sends something here too.
    let large_text = match (input.series.as_str(), input.title.as_str()) {
        ("", "") => "Tchê Flix".to_string(),
        ("", t) => t.to_string(),
        (s, _) => s.to_string(),
    };

    Activity {
        kind,
        details: fit(details),
        state: fit(state),
        large_image: image_url(input),
        large_text: fit(large_text),
        progress: progress(input),
        paused,
        // Part of the diffed view, so flipping the setting mid-playback pushes
        // an update on the next event instead of waiting for the next item.
        status_line: if input.status_title {
            StatusLine::Title
        } else {
            StatusLine::AppName
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn episode() -> Input {
        Input {
            phase: PlaybackPhase::Playing,
            media_type: MediaType::Video,
            title: "Gray Matter".into(),
            series: "Breaking Bad".into(),
            season: "Season 1".into(),
            episode: 5,
            item_id: "abc123".into(),
            duration_us: 3_000_000_000,
            position_us: 1_000_000_000,
            rate: 1.0,
            rating: 0.0,
            year: 0,
            server_url: "https://flix.exemplo.com".into(),
            status_title: true,
        }
    }

    fn movie() -> Input {
        Input {
            series: String::new(),
            season: String::new(),
            episode: 0,
            title: "Blade Runner 2049".into(),
            ..episode()
        }
    }

    #[test]
    fn episode_puts_series_on_top() {
        let a = project(&episode());
        assert_eq!(a.kind, Kind::Episode);
        assert_eq!(a.details, "Breaking Bad");
        assert_eq!(a.state, "🎬 T1:E5 · Gray Matter");
    }

    #[test]
    fn unrated_movie_has_no_second_line() {
        let a = project(&movie());
        assert_eq!(a.kind, Kind::Movie);
        assert_eq!(a.details, "Blade Runner 2049");
        assert_eq!(a.state, "");
    }

    #[test]
    fn movie_puts_score_and_year_on_line_two() {
        let mut i = movie();
        i.rating = 8.0;
        i.year = 2017;
        // No 🎬 here — the ⭐ already opens the line.
        assert_eq!(project(&i).state, "⭐ 8.0 · 2017");
    }

    #[test]
    fn half_a_movie_line_is_still_a_line() {
        let mut i = movie();
        i.rating = 7.95;
        assert_eq!(project(&i).state, "⭐ 8.0");
        let mut i = movie();
        i.year = 2017;
        assert_eq!(project(&i).state, "2017");
    }

    #[test]
    fn nonsense_score_or_year_is_dropped() {
        for (rating, year) in [(0.0, 0), (-1.0, -5), (f64::NAN, 0), (f64::INFINITY, 0)] {
            let mut i = movie();
            i.rating = rating;
            i.year = year;
            assert_eq!(project(&i).state, "", "rating {rating} year {year}");
        }
    }

    #[test]
    fn score_and_year_stay_off_episodes() {
        let mut i = episode();
        i.rating = 8.4;
        i.year = 2022;
        assert_eq!(project(&i).state, "🎬 T1:E5 · Gray Matter");
    }

    #[test]
    fn music_puts_track_on_top() {
        let mut i = episode();
        i.media_type = MediaType::Audio;
        i.title = "Never Gonna Give You Up".into();
        i.series = "Rick Astley".into();
        i.season = "Whenever You Need Somebody".into();
        let a = project(&i);
        assert_eq!(a.kind, Kind::Music);
        assert_eq!(a.details, "Never Gonna Give You Up");
        assert_eq!(a.state, "🎵 Rick Astley · Whenever You Need Somebody");
    }

    #[test]
    fn status_line_follows_the_setting() {
        // On: "Assistindo Breaking Bad" — details is what Discord reads.
        assert_eq!(project(&episode()).status_line, StatusLine::Title);
        let mut off = episode();
        off.status_title = false;
        assert_eq!(project(&off).status_line, StatusLine::AppName);
        // Only the status line changes; the card keeps both lines either way.
        assert_eq!(project(&off).details, "Breaking Bad");
    }

    #[test]
    fn playing_fills_both_edges_of_the_bar() {
        let a = project(&episode());
        let p = a.progress.expect("playing item has a bar");
        assert_eq!(p.elapsed_us, 1_000_000_000);
        assert_eq!(p.remaining_us, 2_000_000_000);
        assert!(!a.paused);
    }

    #[test]
    fn paused_drops_the_bar_and_flags_the_badge() {
        let mut i = episode();
        i.phase = PlaybackPhase::Paused;
        let a = project(&i);
        assert_eq!(a.progress, None);
        assert!(a.paused);
        // The badge carries the pause, so the text is untouched.
        assert_eq!(a.details, "Breaking Bad");
        assert_eq!(a.state, "🎬 T1:E5 · Gray Matter");
    }

    #[test]
    fn stopped_clears_everything_even_with_stale_metadata() {
        let mut i = episode();
        i.phase = PlaybackPhase::Stopped;
        // Metadata survives the end of playback; the projection must not.
        assert_eq!(project(&i), Activity::default());
    }

    #[test]
    fn starting_has_no_bar_yet() {
        let mut i = episode();
        i.phase = PlaybackPhase::Starting;
        assert_eq!(project(&i).progress, None);
    }

    #[test]
    fn rate_scales_both_edges_to_wall_clock() {
        let mut i = episode();
        i.rate = 2.0;
        assert_eq!(
            project(&i).progress,
            Some(Progress {
                elapsed_us: 500_000_000,
                remaining_us: 1_000_000_000,
            })
        );
    }

    #[test]
    fn nonsense_rate_has_no_bar() {
        for rate in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let mut i = episode();
            i.rate = rate;
            assert_eq!(project(&i).progress, None, "rate {rate}");
        }
    }

    #[test]
    fn unknown_duration_has_no_bar() {
        let mut i = episode();
        i.duration_us = 0;
        assert_eq!(project(&i).progress, None);
    }

    #[test]
    fn position_past_end_has_no_bar() {
        let mut i = episode();
        i.position_us = i.duration_us + 1;
        assert_eq!(project(&i).progress, None);
    }

    #[test]
    fn localized_season_still_parses() {
        assert_eq!(season_episode_label("Temporada 3", 7), "T3:E7");
        assert_eq!(season_episode_label("Season 3", 0), "T3");
        assert_eq!(season_episode_label("", 7), "E7");
        assert_eq!(season_episode_label("", 0), "");
    }

    #[test]
    fn private_server_yields_no_image() {
        for host in [
            "http://192.168.1.10:8096",
            "http://127.0.0.1:8096",
            "http://localhost:8096",
            "https://LocalHost/",
            "http://10.0.0.5",
            "http://172.20.0.3",
            "http://172.16.0.1",
            "http://100.72.0.1",
            "http://169.254.1.1",
            "http://jellyfin:8096",
            "http://media.local",
            "http://[::1]:8096",
            "http://[fd00::1]",
            "",
        ] {
            let mut i = episode();
            i.server_url = host.into();
            assert_eq!(project(&i).large_image, "", "{host} should be rejected");
        }
    }

    #[test]
    fn public_server_yields_image_url() {
        for host in [
            "https://flix.exemplo.com",
            "https://flix.exemplo.com/",
            "https://flix.exemplo.com:8920",
            "http://172.32.0.1",
            "https://8.8.8.8",
        ] {
            let mut i = episode();
            i.server_url = host.into();
            let a = project(&i);
            assert!(
                a.large_image.ends_with("/Items/abc123/Images/Primary"),
                "{host} should be accepted, got {:?}",
                a.large_image
            );
            assert!(
                !a.large_image.contains("//Items"),
                "double slash for {host}"
            );
        }
    }

    #[test]
    fn long_fields_are_clamped_to_discord_limit() {
        let mut i = episode();
        i.series = "S".repeat(200);
        i.title = "T".repeat(200);
        let a = project(&i);
        assert_eq!(a.details.chars().count(), DISCORD_FIELD_MAX);
        assert_eq!(a.state.chars().count(), DISCORD_FIELD_MAX);
        assert_eq!(a.large_text.chars().count(), DISCORD_FIELD_MAX);
    }

    #[test]
    fn short_fields_are_padded_to_discord_minimum() {
        let mut i = movie();
        i.title = "Up".into();
        let a = project(&i);
        assert!(a.details.starts_with("Up"));
        assert_eq!(a.details.chars().count(), DISCORD_FIELD_MIN);
        // An empty field must stay empty — the sink reads that as "clear".
        assert_eq!(a.state, "");
    }

    #[test]
    fn large_text_is_never_empty() {
        let mut i = episode();
        i.series = String::new();
        i.title = String::new();
        assert!(!project(&i).large_text.is_empty());
    }

    #[test]
    fn bare_host_override_gets_https() {
        let mut i = episode();
        i.server_url = "  flix.exemplo.com/  ".into();
        assert_eq!(
            project(&i).large_image,
            "https://flix.exemplo.com/Items/abc123/Images/Primary"
        );
    }

    #[test]
    fn missing_item_id_yields_no_image() {
        let mut i = episode();
        i.item_id = String::new();
        assert_eq!(project(&i).large_image, "");
    }
}
