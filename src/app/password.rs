use rocket_dyn_templates::{context, Template};

#[get("/password")]
pub async fn view() -> Template {
	Template::render("password", context! {})
}
