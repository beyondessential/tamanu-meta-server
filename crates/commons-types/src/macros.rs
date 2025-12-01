macro_rules! render_as_string {
	($ty:ty) => {
		render_as_string!($ty, minsize(0))
	};

	($ty:ty, minsize($minsize:literal)) => {
		impl ::tachys::view::Render for $ty {
			type State = ::tachys::view::strings::StringState;

			fn build(self) -> Self::State {
				self.to_string().build()
			}

			fn rebuild(self, state: &mut Self::State) {
				self.to_string().rebuild(state)
			}
		}

		::tachys::no_attrs!($ty);

		impl ::tachys::view::RenderHtml for $ty {
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
				position: &mut ::tachys::view::Position,
				escape: bool,
				mark_branches: bool,
				extra_attrs: Vec<::tachys::html::attribute::any_attribute::AnyAttribute>,
			) {
				self.to_string()
					.to_html_with_buf(buf, position, escape, mark_branches, extra_attrs)
			}

			fn hydrate<const FROM_SERVER: bool>(
				self,
				cursor: &::tachys::hydration::Cursor,
				position: &::tachys::view::PositionState,
			) -> Self::State {
				self.to_string().hydrate::<FROM_SERVER>(cursor, position)
			}

			fn into_owned(self) -> Self::Owned {
				::tachys::view::RenderHtml::into_owned(self.to_string())
			}
		}

		impl ::tachys::html::attribute::IntoAttributeValue for $ty {
			type Output = String;

			fn into_attribute_value(self) -> Self::Output {
				self.to_string()
			}
		}

		impl ::tachys::html::property::IntoProperty for $ty {
			type State = (::web_sys::Element, ::wasm_bindgen::JsValue);
			type Cloneable = ::std::sync::Arc<str>;
			type CloneableOwned = ::std::sync::Arc<str>;

			fn hydrate<const FROM_SERVER: bool>(
				self,
				el: &::web_sys::Element,
				key: &str,
			) -> Self::State {
				::tachys::html::property::IntoProperty::hydrate::<FROM_SERVER>(
					self.to_string(),
					el,
					key,
				)
			}

			fn build(self, el: &::web_sys::Element, key: &str) -> Self::State {
				::tachys::html::property::IntoProperty::build(self.to_string(), el, key)
			}

			fn rebuild(self, state: &mut Self::State, key: &str) {
				::tachys::html::property::IntoProperty::rebuild(self.to_string(), state, key)
			}

			fn into_cloneable(self) -> Self::Cloneable {
				::tachys::html::property::IntoProperty::into_cloneable(self.to_string())
			}

			fn into_cloneable_owned(self) -> Self::CloneableOwned {
				::tachys::html::property::IntoProperty::into_cloneable_owned(self.to_string())
			}
		}
	};
}
pub(crate) use render_as_string;
