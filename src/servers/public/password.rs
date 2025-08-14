use rocket_dyn_templates::{Template, context};

#[get("/password")]
pub async fn view() -> Template {
	Template::render("password", context! {})
}
