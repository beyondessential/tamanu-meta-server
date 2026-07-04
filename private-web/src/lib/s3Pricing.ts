/// AWS S3 Standard storage price (USD per GB-month, first-50TB/month tier),
/// as of 2026-07-01. Source: the AWS Price List API,
/// https://pricing.us-east-1.amazonaws.com/offers/v1.0/aws/AmazonS3/current/<region>/index.json
export const S3_STANDARD_USD_PER_GB_MONTH: Record<string, number> = {
	"us-east-1": 0.023,
	"us-east-2": 0.023,
	"us-west-1": 0.026,
	"us-west-2": 0.023,
	"ca-central-1": 0.025,
	"sa-east-1": 0.0405,
	"eu-west-1": 0.023,
	"eu-west-2": 0.024,
	"eu-west-3": 0.024,
	"eu-central-1": 0.0245,
	"eu-north-1": 0.023,
	"af-south-1": 0.0274,
	"me-south-1": 0.025,
	"ap-southeast-1": 0.025,
	"ap-southeast-2": 0.025,
	"ap-southeast-3": 0.025,
	"ap-northeast-1": 0.025,
	"ap-northeast-2": 0.025,
	"ap-south-1": 0.025,
};

/// AWS S3 data-transfer-OUT-to-internet price, USD per GB (decimal GB, AWS's
/// billing unit), first tier (up to 10 TB/month). Data transfer IN (uploads)
/// is free in every region. Source: AWS's published price list
/// (`AWSDataTransfer` service, https://aws.amazon.com/s3/pricing/), fetched
/// 2026-07-04 — re-check before relying on this for real budgeting.
export const S3_EGRESS_USD_PER_GB: Record<string, number> = {
	"us-east-1": 0.09,
	"us-west-2": 0.09,
	"eu-west-1": 0.09,
	"ap-southeast-1": 0.12,
	"ap-southeast-2": 0.114,
	"ap-southeast-3": 0.132,
	"ap-southeast-4": 0.114,
	"ap-northeast-1": 0.114,
};

/// Region assumed when a group's backup config has no explicit region set.
/// The server resolves a NULL region to the deployment pod's own
/// `AWS_REGION`, which the frontend can't see; the fleet is predominantly
/// Pacific/Australia, so assume its home region and label the estimate.
export const DEFAULT_S3_REGION = "ap-southeast-2";

/// Estimated monthly S3 Standard storage cost for a bucket, as a compact
/// tooltip string. Uses the single-rate first tier (fleet buckets are nowhere
/// near the 50TB tiering threshold), so this is always an approximation.
export function estimatedBucketCostTooltip(
	bucketBytes: number,
	region: string | null | undefined,
): string {
	const knownRate = region != null ? S3_STANDARD_USD_PER_GB_MONTH[region] : undefined;
	const rate = knownRate ?? S3_STANDARD_USD_PER_GB_MONTH[DEFAULT_S3_REGION];
	const gb = bucketBytes / 1024 ** 3;
	const usd = gb * rate;
	const regionLabel =
		knownRate != null
			? region
			: `estimated, ${region ? `unrecognised region "${region}"` : "no region set"}`;
	return `~$${usd.toFixed(2)}/month at $${rate}/GB (${regionLabel})`;
}

/// Egress rate to use for a group's cost estimate: the region's own rate if
/// known, else `DEFAULT_S3_REGION`'s. `assumed` is true when `region` itself
/// was `null` (config default) rather than an explicit, just-unlisted region.
export function s3EgressRateForRegion(region: string | null): {
	rate: number;
	region: string;
	assumed: boolean;
} {
	const effective = region ?? DEFAULT_S3_REGION;
	const rate =
		S3_EGRESS_USD_PER_GB[effective] ?? S3_EGRESS_USD_PER_GB[DEFAULT_S3_REGION];
	return { rate, region: effective, assumed: region == null };
}
