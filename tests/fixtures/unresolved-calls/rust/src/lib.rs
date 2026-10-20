pub struct Widget;

impl Widget {
    pub fn render(&self) -> u32 {
        1
    }

    pub fn build() -> Widget {
        Widget
    }
}

pub fn use_widget() -> u32 {
    let widget = Widget::build();
    widget.render()
}

pub fn run(callback: impl Fn() -> u32) -> u32 {
    callback()
}

#[cfg(any())]
pub fn gated(value: Widget) -> u32 {
    let _built = Widget::build();
    value.render()
}
