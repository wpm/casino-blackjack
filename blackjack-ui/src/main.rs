use leptos::prelude::*;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    view! {
        <main style="display:flex;align-items:center;justify-content:center;height:100vh;background:#0b5d2a;color:#e8e3d3;font-family:Georgia,serif;">
            <p>"Casino Blackjack"</p>
        </main>
    }
}
