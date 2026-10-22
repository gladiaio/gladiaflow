use serde::Serialize;
use url::Url;

#[cfg(target_os = "macos")]
use chrono::{Duration, Utc};
#[cfg(target_os = "macos")]
use highlandcows_eventkit::{CalendarStore, EkAuthStatus};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarMeeting {
    id: String,
    title: String,
    start_at: String,
    end_at: String,
    location: Option<String>,
    join_url: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSnapshot {
    status: &'static str,
    meetings: Vec<CalendarMeeting>,
}

#[cfg(target_os = "macos")]
fn status_name(status: EkAuthStatus) -> &'static str {
    match status {
        EkAuthStatus::NotDetermined => "not_determined",
        EkAuthStatus::Restricted => "restricted",
        EkAuthStatus::Denied => "denied",
        EkAuthStatus::FullAccess => "authorized",
        EkAuthStatus::WriteOnly => "write_only",
    }
}

fn meeting_url(notes: Option<&str>, location: Option<&str>) -> Option<String> {
    notes
        .into_iter()
        .chain(location)
        .flat_map(str::split_whitespace)
        .map(|candidate| {
            candidate.trim_matches(|character: char| {
                matches!(
                    character,
                    '<' | '>' | '(' | ')' | '[' | ']' | ',' | ';' | '.' | '"' | '\''
                )
            })
        })
        .find_map(|candidate| {
            let parsed = Url::parse(candidate).ok()?;
            (parsed.scheme() == "https" && parsed.host_str().is_some()).then(|| parsed.to_string())
        })
}

#[cfg(target_os = "macos")]
pub fn meetings(request_access: bool) -> Result<CalendarSnapshot, String> {
    let macos_major = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|version| version.split('.').next()?.trim().parse::<u32>().ok());
    if macos_major.is_some_and(|major| major < 14) {
        return Ok(CalendarSnapshot {
            status: "unsupported",
            meetings: Vec::new(),
        });
    }

    let status = CalendarStore::authorization_status();
    if !request_access && status != EkAuthStatus::FullAccess {
        return Ok(CalendarSnapshot {
            status: status_name(status),
            meetings: Vec::new(),
        });
    }

    let store = CalendarStore::builder()
        .connect()
        .map_err(|error| error.to_string())?;
    let token = store.authorize().map_err(|error| error.to_string())?;
    let now = Utc::now();
    let events = store
        .fetch_in_range(
            now - Duration::hours(4),
            now + Duration::days(1),
            None,
            &token,
        )
        .map_err(|error| error.to_string())?;
    let mut meetings = events
        .into_iter()
        .filter_map(|event| {
            let start_at = event.start_date?;
            let end_at = event.end_date?;
            if event.is_all_day || end_at < now {
                return None;
            }
            Some(CalendarMeeting {
                id: event
                    .identifier
                    .unwrap_or_else(|| format!("{}-{}", event.title, start_at.timestamp())),
                join_url: meeting_url(event.notes.as_deref(), event.location.as_deref()),
                title: event.title,
                start_at: start_at.to_rfc3339(),
                end_at: end_at.to_rfc3339(),
                location: event.location,
            })
        })
        .collect::<Vec<_>>();
    meetings.sort_by(|left, right| left.start_at.cmp(&right.start_at));
    Ok(CalendarSnapshot {
        status: "authorized",
        meetings,
    })
}

#[cfg(not(target_os = "macos"))]
pub fn meetings(_request_access: bool) -> Result<CalendarSnapshot, String> {
    Ok(CalendarSnapshot {
        status: "unsupported",
        meetings: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::meeting_url;

    #[test]
    fn extracts_https_meeting_urls_from_local_event_fields() {
        assert_eq!(
            meeting_url(
                Some("Join at https://meet.example.com/room)."),
                Some("Conference room")
            )
            .as_deref(),
            Some("https://meet.example.com/room")
        );
        assert_eq!(meeting_url(Some("file:///tmp/private"), None), None);
    }
}
