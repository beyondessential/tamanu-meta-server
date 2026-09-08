# Review Hero on the Canopy repo

Verification is manual: Review Hero runs as a GitHub Actions workflow, so the scenarios below are exercised by opening pull requests against this repository.

Config, prompts, and rules are read from the **default branch**, not the PR branch. Until this card's branch merges, a review running on it uses built-in defaults and ignores `.github/review-hero/config.yml`, unless the `REVIEW_HERO_BOOTSTRAP_BRANCH` repo secret names the branch.

## Trigger

- [ ] Opening a pull request does not start a review while the Review Hero checkbox is unticked.
- [ ] Ticking **Run Review Hero** in the pull request body starts a review and it posts its findings on the pull request.
- [ ] The checkbox is unticked again after the review posts, so a later tick runs a fresh review.
- [ ] A new pull request is created with the Review Hero checkbox already present, from the pull request template.

## Credentials

- [ ] The review authenticates as the Review Hero GitHub App and its comments are attributed to that app rather than to a user.
- [ ] No workflow run fails for a missing `REVIEW_HERO_APP_ID`, `REVIEW_HERO_PRIVATE_KEY`, or Anthropic API key.

## Configuration

- [ ] A review on a pull request touching `crates/canopy-api/src/generated.rs`, either `openapi.json`, or `private-web/src/api-types.ts` reports no findings against those files.
- [ ] That same review still reports findings against hand-written neighbours such as `crates/canopy-api/src/client.rs`, `crates/canopy-api/tests/`, and `private-web/src/types.ts`.
- [ ] A review reflects the conventions in `AGENTS.md`, which the agent reads natively via `CLAUDE.md` rather than from Review Hero's config.

## Interaction with existing CI

- [ ] Editing a pull request body to tick the checkbox does not re-run `ci.yml`, whose triggers are unchanged.
- [ ] A Review Hero run neither blocks nor is blocked by the merge queue.
