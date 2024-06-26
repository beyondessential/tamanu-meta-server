use diesel::{backend::Backend, deserialize, expression::AsExpression, serialize, sql_types::Text};
use rocket::{http::Header, serde::Serialize};
use rocket_db_pools::{diesel::PgPool, Database};
use rocket_dyn_templates::Template;

#[derive(Database)]
#[database("postgres")]
pub struct Db(PgPool);

#[derive(Debug, Responder)]
pub struct TamanuHeaders<T> {
	inner: T,
	version: Version,
	server_type: ServerType,
}

impl<T> TamanuHeaders<T> {
	pub fn new(inner: T) -> Self {
		Self {
			inner,
			server_type: ServerType,
			version: Version(node_semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap()),
		}
	}
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, AsExpression)]
#[diesel(sql_type = Text)]
pub struct Version(pub node_semver::Version);

impl From<Version> for Header<'_> {
	fn from(version: Version) -> Self {
		Header::new("X-Tamanu-Version", version.0.to_string())
	}
}

impl<DB> deserialize::FromSql<Text, DB> for Version
where
	DB: Backend,
	String: deserialize::FromSql<Text, DB>,
{
	fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
		String::from_sql(bytes).and_then(|string| {
			node_semver::Version::parse(string)
				.map(Version)
				.map_err(|err| Box::new(err) as Box<dyn std::error::Error + Send + Sync>)
		})
	}
}

impl serialize::ToSql<Text, diesel::pg::Pg> for Version
where
	String: serialize::ToSql<Text, diesel::pg::Pg>,
{
	fn to_sql<'b>(
		&'b self,
		out: &mut serialize::Output<'b, '_, diesel::pg::Pg>,
	) -> serialize::Result {
		let v = self.0.to_string();
		<String as serialize::ToSql<Text, diesel::pg::Pg>>::to_sql(&v, &mut out.reborrow())
	}
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ServerType;

impl From<ServerType> for Header<'_> {
	fn from(_: ServerType) -> Self {
		Header::new("X-Tamanu-Server", "Tamanu Metadata Server")
	}
}

#[catch(404)]
fn not_found() -> TamanuHeaders<()> {
	TamanuHeaders::new(())
}

#[launch]
pub fn rocket() -> _ {
	rocket::build()
		.attach(Template::fairing())
		.attach(Db::init())
		.register("/", catchers![not_found])
		.mount(
			"/",
			routes![
				crate::servers::list,
				crate::servers::create,
				crate::servers::edit,
				crate::statuses::view,
				crate::versions::view
			],
		)
}
