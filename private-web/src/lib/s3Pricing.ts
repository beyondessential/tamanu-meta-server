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

/// Region assumed when a group's backup config has none set. The fleet is
/// predominantly Pacific/Australia, so this is a best-guess, not a promise —
/// the actual region a `null` config resolves to is an operator/deployment
/// concern outside this config row.
export const DEFAULT_S3_REGION = "ap-southeast-2";

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
