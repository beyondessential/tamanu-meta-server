use std::str::FromStr;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum CloudRegion {
	AwsAuckland,
	AwsCalifornia,
	AwsCanada,
	AwsFrankfurt,
	AwsLondon,
	AwsMumbai,
	AwsNorthVirginia,
	AwsOhio,
	AwsOregon,
	AwsParis,
	AwsSaoPaulo,
	AwsSeoul,
	AwsSingapore,
	AwsSydney,
	AwsTokyo,
	AwsZurich,
	AzureAustraliaEast,
	AzureEastUs,
	AzureSoutheastAsia,
	AzureWestEurope,
}

impl CloudRegion {
	pub const ALL: [Self; 20] = [
		Self::AwsAuckland,
		Self::AwsCalifornia,
		Self::AwsCanada,
		Self::AwsFrankfurt,
		Self::AwsLondon,
		Self::AwsMumbai,
		Self::AwsNorthVirginia,
		Self::AwsOhio,
		Self::AwsOregon,
		Self::AwsParis,
		Self::AwsSaoPaulo,
		Self::AwsSeoul,
		Self::AwsSingapore,
		Self::AwsSydney,
		Self::AwsTokyo,
		Self::AwsZurich,
		Self::AzureAustraliaEast,
		Self::AzureEastUs,
		Self::AzureSoutheastAsia,
		Self::AzureWestEurope,
	];

	pub fn to_lat_lon(self) -> (f64, f64) {
		match self {
			CloudRegion::AwsAuckland => (-36.8485, 174.7633),
			CloudRegion::AwsCalifornia => (37.7749, -122.4194),
			CloudRegion::AwsCanada => (45.5017, -73.5673),
			CloudRegion::AwsFrankfurt => (50.110922, 8.682127),
			CloudRegion::AwsLondon => (51.507351, -0.127758),
			CloudRegion::AwsMumbai => (19.075984, 72.877656),
			CloudRegion::AwsNorthVirginia => (38.9072, -77.0369),
			CloudRegion::AwsOhio => (39.9612, -82.9988),
			CloudRegion::AwsOregon => (45.5152, -122.6784),
			CloudRegion::AwsParis => (48.856614, 2.352222),
			CloudRegion::AwsSaoPaulo => (-23.5505, -46.6333),
			CloudRegion::AwsSeoul => (37.566535, 126.977969),
			CloudRegion::AwsSingapore => (1.352083, 103.819836),
			CloudRegion::AwsSydney => (-33.868820, 151.209296),
			CloudRegion::AwsTokyo => (35.689487, 139.691706),
			CloudRegion::AwsZurich => (47.376887, 8.541694),
			CloudRegion::AzureAustraliaEast => (-33.8688, 151.2093),
			CloudRegion::AzureEastUs => (37.3719, -79.8164),
			CloudRegion::AzureSoutheastAsia => (1.283333, 103.833333),
			CloudRegion::AzureWestEurope => (52.3667, 4.8945),
		}
	}

	// TODO: make fuzzy
	pub fn from_lat_lon(lat: f64, lon: f64) -> Option<Self> {
		match (lat, lon) {
			(-36.8485, 174.7633) => Some(CloudRegion::AwsAuckland),
			(50.110922, 8.682127) => Some(CloudRegion::AwsFrankfurt),
			(37.7749, -122.4194) => Some(CloudRegion::AwsCalifornia),
			(45.5017, -73.5673) => Some(CloudRegion::AwsCanada),
			(51.507351, -0.127758) => Some(CloudRegion::AwsLondon),
			(19.075984, 72.877656) => Some(CloudRegion::AwsMumbai),
			(38.9072, -77.0369) => Some(CloudRegion::AwsNorthVirginia),
			(39.9612, -82.9988) => Some(CloudRegion::AwsOhio),
			(45.5152, -122.6784) => Some(CloudRegion::AwsOregon),
			(48.856614, 2.352222) => Some(CloudRegion::AwsParis),
			(-23.5505, -46.6333) => Some(CloudRegion::AwsSaoPaulo),
			(37.566535, 126.977969) => Some(CloudRegion::AwsSeoul),
			(1.352083, 103.819836) => Some(CloudRegion::AwsSingapore),
			(-33.868820, 151.209296) => Some(CloudRegion::AwsSydney),
			(35.689487, 139.691706) => Some(CloudRegion::AwsTokyo),
			(47.376887, 8.541694) => Some(CloudRegion::AwsZurich),
			(-33.8688, 151.2093) => Some(CloudRegion::AzureAustraliaEast),
			(1.283333, 103.833333) => Some(CloudRegion::AzureSoutheastAsia),
			(52.3667, 4.8945) => Some(CloudRegion::AzureWestEurope),
			(37.3719, -79.8164) => Some(CloudRegion::AzureEastUs),
			_ => None,
		}
	}

	pub fn as_str(self) -> &'static str {
		match self {
			CloudRegion::AwsAuckland => "AWS Auckland",
			CloudRegion::AwsCalifornia => "AWS California",
			CloudRegion::AwsCanada => "AWS Canada",
			CloudRegion::AwsFrankfurt => "AWS Frankfurt",
			CloudRegion::AwsLondon => "AWS London",
			CloudRegion::AwsMumbai => "AWS Mumbai",
			CloudRegion::AwsNorthVirginia => "AWS North Virginia",
			CloudRegion::AwsOhio => "AWS Ohio",
			CloudRegion::AwsOregon => "AWS Oregon",
			CloudRegion::AwsParis => "AWS Paris",
			CloudRegion::AwsSaoPaulo => "AWS São Paulo",
			CloudRegion::AwsSeoul => "AWS Seoul",
			CloudRegion::AwsSingapore => "AWS Singapore",
			CloudRegion::AwsSydney => "AWS Sydney",
			CloudRegion::AwsTokyo => "AWS Tokyo",
			CloudRegion::AwsZurich => "AWS Zurich",
			CloudRegion::AzureAustraliaEast => "Azure Australia East",
			CloudRegion::AzureEastUs => "Azure East US",
			CloudRegion::AzureSoutheastAsia => "Azure Southeast Asia",
			CloudRegion::AzureWestEurope => "Azure West Europe",
		}
	}
}

impl FromStr for CloudRegion {
	type Err = ();

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"AWS Auckland" => Ok(CloudRegion::AwsAuckland),
			"AWS California" => Ok(CloudRegion::AwsCalifornia),
			"AWS Canada" => Ok(CloudRegion::AwsCanada),
			"AWS Frankfurt" => Ok(CloudRegion::AwsFrankfurt),
			"AWS London" => Ok(CloudRegion::AwsLondon),
			"AWS Mumbai" => Ok(CloudRegion::AwsMumbai),
			"AWS North Virginia" => Ok(CloudRegion::AwsNorthVirginia),
			"AWS Ohio" => Ok(CloudRegion::AwsOhio),
			"AWS Oregon" => Ok(CloudRegion::AwsOregon),
			"AWS Paris" => Ok(CloudRegion::AwsParis),
			"AWS Seoul" => Ok(CloudRegion::AwsSeoul),
			"AWS Singapore" => Ok(CloudRegion::AwsSingapore),
			"AWS Sydney" => Ok(CloudRegion::AwsSydney),
			"AWS São Paulo" => Ok(CloudRegion::AwsSaoPaulo),
			"AWS Tokyo" => Ok(CloudRegion::AwsTokyo),
			"AWS Zurich" => Ok(CloudRegion::AwsZurich),
			"Azure Australia East" => Ok(CloudRegion::AzureAustraliaEast),
			"Azure East US" => Ok(CloudRegion::AzureEastUs),
			"Azure Southeast Asia" => Ok(CloudRegion::AzureSoutheastAsia),
			"Azure West Europe" => Ok(CloudRegion::AzureWestEurope),
			_ => Err(()),
		}
	}
}
