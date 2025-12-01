pub use tachys;
pub use wasm_bindgen;
pub use web_sys;

#[macro_export]
macro_rules! render_as_string {
	($ty:ty) => {
		$crate::render_as_string!($ty, minsize(0));
	};

	($ty:ty, minsize($minsize:literal)) => {
		impl $crate::tachys::view::Render for $ty {
			type State = $crate::tachys::view::strings::StringState;

			fn build(self) -> Self::State {
				self.to_string().build()
			}

			fn rebuild(self, state: &mut Self::State) {
				self.to_string().rebuild(state)
			}
		}

		$crate::tachys::no_attrs!($ty);

		impl $crate::tachys::view::RenderHtml for $ty {
			const MIN_LENGTH: usize = $minsize;
			type AsyncOutput = String;
			type Owned = String;

			fn dry_resolve(&mut self) {
				self.to_string().dry_resolve()
			}

			async fn resolve(self) -> Self::AsyncOutput {
				self.to_string().resolve().await
			}

			fn html_len(&self) -> usize {
				self.to_string().html_len()
			}

			fn to_html_with_buf(
				self,
				buf: &mut String,
				position: &mut $crate::tachys::view::Position,
				escape: bool,
				mark_branches: bool,
				extra_attrs: Vec<$crate::tachys::html::attribute::any_attribute::AnyAttribute>,
			) {
				self.to_string()
					.to_html_with_buf(buf, position, escape, mark_branches, extra_attrs)
			}

			fn hydrate<const FROM_SERVER: bool>(
				self,
				cursor: &$crate::tachys::hydration::Cursor,
				position: &$crate::tachys::view::PositionState,
			) -> Self::State {
				self.to_string().hydrate::<FROM_SERVER>(cursor, position)
			}

			fn into_owned(self) -> Self::Owned {
				$crate::tachys::view::RenderHtml::into_owned(self.to_string())
			}
		}

		impl $crate::tachys::html::attribute::IntoAttributeValue for $ty {
			type Output = String;

			fn into_attribute_value(self) -> Self::Output {
				self.to_string()
			}
		}

		impl $crate::tachys::html::property::IntoProperty for $ty {
			type State = ($crate::web_sys::Element, $crate::wasm_bindgen::JsValue);
			type Cloneable = ::std::sync::Arc<str>;
			type CloneableOwned = ::std::sync::Arc<str>;

			fn hydrate<const FROM_SERVER: bool>(
				self,
				el: &$crate::web_sys::Element,
				key: &str,
			) -> Self::State {
				$crate::tachys::html::property::IntoProperty::hydrate::<FROM_SERVER>(
					self.to_string(),
					el,
					key,
				)
			}

			fn build(self, el: &$crate::web_sys::Element, key: &str) -> Self::State {
				$crate::tachys::html::property::IntoProperty::build(self.to_string(), el, key)
			}

			fn rebuild(self, state: &mut Self::State, key: &str) {
				$crate::tachys::html::property::IntoProperty::rebuild(self.to_string(), state, key)
			}

			fn into_cloneable(self) -> Self::Cloneable {
				$crate::tachys::html::property::IntoProperty::into_cloneable(self.to_string())
			}

			fn into_cloneable_owned(self) -> Self::CloneableOwned {
				$crate::tachys::html::property::IntoProperty::into_cloneable_owned(self.to_string())
			}
		}

		impl $crate::tachys::html::class::IntoClass for $ty {
			type AsyncOutput = String;
			type State = ($crate::web_sys::Element, String);
			type Cloneable = ::std::sync::Arc<str>;
			type CloneableOwned = ::std::sync::Arc<str>;

			fn html_len(&self) -> usize {
				$crate::tachys::html::class::IntoClass::html_len(&self.to_string())
			}

			fn to_html(self, class: &mut String) {
				$crate::tachys::html::class::IntoClass::to_html(self.to_string(), class)
			}

			fn should_overwrite(&self) -> bool {
				true
			}

			fn hydrate<const FROM_SERVER: bool>(
				self,
				el: &$crate::web_sys::Element,
			) -> Self::State {
				$crate::tachys::html::class::IntoClass::hydrate::<FROM_SERVER>(self.to_string(), el)
			}

			fn build(self, el: &$crate::web_sys::Element) -> Self::State {
				$crate::tachys::html::class::IntoClass::build(self.to_string(), el)
			}

			fn rebuild(self, state: &mut Self::State) {
				$crate::tachys::html::class::IntoClass::rebuild(self.to_string(), state)
			}

			fn into_cloneable(self) -> Self::Cloneable {
				$crate::tachys::html::class::IntoClass::into_cloneable(self.to_string())
			}

			fn into_cloneable_owned(self) -> Self::CloneableOwned {
				$crate::tachys::html::class::IntoClass::into_cloneable_owned(self.to_string())
			}

			fn dry_resolve(&mut self) {
				$crate::tachys::html::class::IntoClass::dry_resolve(&mut self.to_string())
			}

			async fn resolve(self) -> Self::AsyncOutput {
				$crate::tachys::html::class::IntoClass::resolve(self.to_string()).await
			}

			fn reset(state: &mut Self::State) {
				let (el, _prev) = state;
				$crate::tachys::renderer::Rndr::remove_attribute(el, "class");
			}
		}
	};
}
