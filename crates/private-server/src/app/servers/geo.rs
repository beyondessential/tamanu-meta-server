use std::str::FromStr;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum CloudRegion {
	AwsSydney,
	AwsZurich,
}

impl CloudRegion {
	pub const ALL: [Self; 2] = [Self::AwsSydney, Self::AwsZurich];

	pub fn to_lat_lon(self) -> (f64, f64) {
		match self {
			CloudRegion::AwsSydney => (-33.868820, 151.209296),
			CloudRegion::AwsZurich => (47.376887, 8.541694),
		}
	}

	// TODO: make fuzzy
	pub fn from_lat_lon(lat: f64, lon: f64) -> Option<Self> {
		match (lat, lon) {
			(-33.868820, 151.209296) => Some(CloudRegion::AwsSydney),
			(47.376887, 8.541694) => Some(CloudRegion::AwsZurich),
			_ => None,
		}
	}

	pub fn as_str(self) -> &'static str {
		match self {
			CloudRegion::AwsSydney => "AWS Sydney",
			CloudRegion::AwsZurich => "AWS Zurich",
		}
	}
}

impl FromStr for CloudRegion {
	type Err = ();

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"AWS Sydney" => Ok(CloudRegion::AwsSydney),
			"AWS Zurich" => Ok(CloudRegion::AwsZurich),
			_ => Err(()),
		}
	}
}
