use std::path::Path;
use std::process::Command;
use std::{env, fs};

fn main() {
	let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
	let frontend = Path::new(&manifest_dir).join("../../private-web");
	let dist = frontend.join("dist");

	println!("cargo:rerun-if-changed=../../private-web/src");
	println!("cargo:rerun-if-changed=../../private-web/public");
	println!("cargo:rerun-if-changed=../../private-web/package.json");
	println!("cargo:rerun-if-changed=../../private-web/package-lock.json");
	println!("cargo:rerun-if-changed=../../private-web/vite.config.ts");
	println!("cargo:rerun-if-changed=../../private-web/index.html");
	println!("cargo:rerun-if-changed=../../private-web/tsconfig.app.json");
	println!("cargo:rerun-if-env-changed=SKIP_FRONTEND_BUILD");

	// Make sure dist/ always exists so rust-embed has something to point at,
	// even when we skip the build below.
	fs::create_dir_all(&dist).expect("failed to create private-web/dist");

	if env::var("SKIP_FRONTEND_BUILD").is_ok() {
		// Dev path: trust whatever is on disk. The vite dev server is the
		// real source of UI; the rust binary's embedded assets are only
		// hit in prod.
		return;
	}

	let Some(npm) = which_npm() else {
		// No npm available — assume someone built the frontend out of band
		// (e.g. CI) and dropped the dist into place.
		println!(
			"cargo:warning=npm not found; using whatever is in private-web/dist (set SKIP_FRONTEND_BUILD=1 to silence this)"
		);
		return;
	};

	let status = Command::new(&npm)
		.args(["install", "--frozen-lockfile"])
		.current_dir(&frontend)
		.status()
		.expect("failed to run npm install");
	assert!(status.success(), "npm install failed");

	let status = Command::new(&npm)
		.args(["run", "build"])
		.current_dir(&frontend)
		.status()
		.expect("failed to run npm run build");
	assert!(status.success(), "npm run build failed");
}

fn which_npm() -> Option<String> {
	for candidate in ["npm", "npm.cmd"] {
		if Command::new(candidate)
			.arg("--version")
			.output()
			.is_ok_and(|o| o.status.success())
		{
			return Some(candidate.to_owned());
		}
	}
	None
}
