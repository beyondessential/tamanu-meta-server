//! Input validation on `NewEvent::save`: the `description` field is a
//! single-line title, multi-line content belongs in `message`.

use commons_errors::AppError;
use database::issues::NewEvent;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread")]
async fn save_rejects_newline_in_description() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let event = NewEvent {
			source: "test".into(),
			r#ref: "ref-newline".into(),
			description: Some("line one\nline two".into()),
			message: "body".into(),
			active: Some(true),
			occurred_at: None,
		};
		let err = event
			.save(&mut conn, Uuid::nil(), None)
			.await
			.expect_err("description with newline must be rejected");
		match err {
			AppError::BadRequest(_) => {}
			other => panic!("expected BadRequest, got {other:?}"),
		}
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn save_accepts_single_line_description() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let event = NewEvent {
			source: "test".into(),
			r#ref: "ref-singleline".into(),
			description: Some("a perfectly fine subject line".into()),
			message: "body".into(),
			active: Some(true),
			occurred_at: None,
		};
		event
			.save(&mut conn, Uuid::nil(), None)
			.await
			.expect("single-line description saves");
	})
	.await
}
