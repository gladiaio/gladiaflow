pub fn detect_region() -> &'static str {
    let tz = iana_time_zone::get_timezone().unwrap_or_default();
    region_from_timezone(&tz)
}

fn region_from_timezone(tz: &str) -> &'static str {
    if tz.starts_with("America/")
        || tz.starts_with("US/")
        || tz.starts_with("Canada/")
        || tz.starts_with("Mexico/")
    {
        "us-west"
    } else {
        "eu-west"
    }
}

#[cfg(test)]
mod tests {
    use super::region_from_timezone;

    // --- Americas → us-west ---

    #[test]
    fn america_new_york() {
        assert_eq!(region_from_timezone("America/New_York"), "us-west");
    }

    #[test]
    fn america_los_angeles() {
        assert_eq!(region_from_timezone("America/Los_Angeles"), "us-west");
    }

    #[test]
    fn america_chicago() {
        assert_eq!(region_from_timezone("America/Chicago"), "us-west");
    }

    #[test]
    fn america_denver() {
        assert_eq!(region_from_timezone("America/Denver"), "us-west");
    }

    #[test]
    fn america_toronto() {
        assert_eq!(region_from_timezone("America/Toronto"), "us-west");
    }

    #[test]
    fn america_vancouver() {
        assert_eq!(region_from_timezone("America/Vancouver"), "us-west");
    }

    #[test]
    fn america_mexico_city() {
        assert_eq!(region_from_timezone("America/Mexico_City"), "us-west");
    }

    #[test]
    fn america_sao_paulo() {
        assert_eq!(region_from_timezone("America/Sao_Paulo"), "us-west");
    }

    #[test]
    fn america_bogota() {
        assert_eq!(region_from_timezone("America/Bogota"), "us-west");
    }

    #[test]
    fn us_pacific() {
        assert_eq!(region_from_timezone("US/Pacific"), "us-west");
    }

    #[test]
    fn us_eastern() {
        assert_eq!(region_from_timezone("US/Eastern"), "us-west");
    }

    #[test]
    fn us_central() {
        assert_eq!(region_from_timezone("US/Central"), "us-west");
    }

    #[test]
    fn canada_eastern() {
        assert_eq!(region_from_timezone("Canada/Eastern"), "us-west");
    }

    #[test]
    fn canada_pacific() {
        assert_eq!(region_from_timezone("Canada/Pacific"), "us-west");
    }

    #[test]
    fn mexico_general() {
        assert_eq!(region_from_timezone("Mexico/General"), "us-west");
    }

    // --- Europe → eu-west ---

    #[test]
    fn europe_paris() {
        assert_eq!(region_from_timezone("Europe/Paris"), "eu-west");
    }

    #[test]
    fn europe_london() {
        assert_eq!(region_from_timezone("Europe/London"), "eu-west");
    }

    #[test]
    fn europe_berlin() {
        assert_eq!(region_from_timezone("Europe/Berlin"), "eu-west");
    }

    #[test]
    fn europe_amsterdam() {
        assert_eq!(region_from_timezone("Europe/Amsterdam"), "eu-west");
    }

    #[test]
    fn europe_warsaw() {
        assert_eq!(region_from_timezone("Europe/Warsaw"), "eu-west");
    }

    #[test]
    fn europe_kiev() {
        assert_eq!(region_from_timezone("Europe/Kiev"), "eu-west");
    }

    // --- Asia → eu-west ---

    #[test]
    fn asia_tokyo() {
        assert_eq!(region_from_timezone("Asia/Tokyo"), "eu-west");
    }

    #[test]
    fn asia_kolkata() {
        assert_eq!(region_from_timezone("Asia/Kolkata"), "eu-west");
    }

    #[test]
    fn asia_dubai() {
        assert_eq!(region_from_timezone("Asia/Dubai"), "eu-west");
    }

    #[test]
    fn asia_singapore() {
        assert_eq!(region_from_timezone("Asia/Singapore"), "eu-west");
    }

    // --- Africa → eu-west ---

    #[test]
    fn africa_lagos() {
        assert_eq!(region_from_timezone("Africa/Lagos"), "eu-west");
    }

    #[test]
    fn africa_johannesburg() {
        assert_eq!(region_from_timezone("Africa/Johannesburg"), "eu-west");
    }

    // --- Pacific → eu-west ---

    #[test]
    fn pacific_auckland() {
        assert_eq!(region_from_timezone("Pacific/Auckland"), "eu-west");
    }

    #[test]
    fn pacific_sydney() {
        assert_eq!(region_from_timezone("Australia/Sydney"), "eu-west");
    }

    // --- Edge cases ---

    #[test]
    fn empty_string_defaults_to_eu_west() {
        assert_eq!(region_from_timezone(""), "eu-west");
    }

    #[test]
    fn unknown_timezone_defaults_to_eu_west() {
        assert_eq!(region_from_timezone("Unknown/Timezone"), "eu-west");
    }

    #[test]
    fn utc_defaults_to_eu_west() {
        assert_eq!(region_from_timezone("UTC"), "eu-west");
    }

    #[test]
    fn prefix_only_no_slash_is_not_matched() {
        // "America" without a trailing "/" must not match "America/"
        assert_eq!(region_from_timezone("America"), "eu-west");
    }
}
