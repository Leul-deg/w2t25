use yew::prelude::*;
use crate::components::nav::Nav;

#[derive(Properties, PartialEq)]
pub struct LayoutProps {
    pub children: Children,
    pub on_logout: Callback<()>,
}

#[function_component(Layout)]
pub fn layout(props: &LayoutProps) -> Html {
    html! {
        <>
            <Nav on_logout={props.on_logout.clone()} />
            <main class="meridian-main">
                { for props.children.iter() }
            </main>
        </>
    }
}
