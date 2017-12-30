use std::time::Duration;

pub(crate) trait DurationExtensions {
    // Rust beta has a from_micros function that is unstable.
    fn from_micros_ext(u64) -> Duration;
    fn as_millis(&self) -> u64;
    fn as_micros(&self) -> u64;
}

impl DurationExtensions for Duration {
    fn from_micros_ext(micros: u64) -> Duration {
        let whole_seconds = micros / 1_000_000;
        let rest = (micros - (whole_seconds * 1_000_000)) as u32;
        Duration::new(whole_seconds, rest * 1000)
    }

    fn as_millis(&self) -> u64 {
        self.as_secs() * 1000 + u64::from(self.subsec_nanos() / 1000 / 1000)
    }

    fn as_micros(&self) -> u64 {
        self.as_secs() * 1000 * 1000 + u64::from(self.subsec_nanos() / 1000)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn it_constructs_durations_from_micros() {
        let expected = Duration::new(5, 543_210_000);
        let actual = Duration::from_micros_ext(5_543_210);
        assert_eq!(actual, expected);
    }

    #[test]
    fn it_calculates_whole_millis_from_durations() {
        let duration = Duration::new(5, 543_210_000);
        assert_eq!(duration.as_millis(), 5543);
    }

    #[test]
    fn it_calculates_whole_micros_from_durations() {
        let duration = Duration::new(5, 543_210_000);
        assert_eq!(duration.as_micros(), 5_543_210);
    }
}