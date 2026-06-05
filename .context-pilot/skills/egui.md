---
name: egui
description: egui framework knowledge and patterns
---
# SKILL.md — egui 0.34.1 Complete Reference for LLM Code Generation

**egui 0.34.1** is an immediate-mode Rust GUI library where the entire UI rebuilds every frame — no retained widget tree, no callbacks. Version 0.34 introduced a **major paradigm shift**: `Ui` replaces `Context` as the primary entry point, with `App::update` deprecated in favor of `App::ui`. The default renderer switched from `glow` to `wgpu`, and font rendering moved from `ab_glyph` to **skrifa + vello_cpu** with font hinting enabled.

---

## Cargo.toml setup

```toml
[package]
name = "my_egui_app"
version = "0.1.0"
edition = "2021"

[dependencies]
eframe = { version = "0.34.1", default-features = false, features = [
    "default_fonts",  # Bundle default egui fonts
    "wgpu",           # wgpu rendering (NEW DEFAULT in 0.34). Alt: "glow"
    "persistence",    # Save/restore app state to disk (enables serde, ron)
    "wayland",        # Wayland support (Linux)
    "x11",            # X11 support (Linux)
    # "accesskit",    # Screen reader support (many deps)
] }
egui = "0.34"
log = "0.4"
serde = { version = "1", features = ["derive"] }  # Only if using persistence

# Optional companion crates:
# egui_plot = "0.34"       # 2D plotting
# egui_extras = "0.34"     # Tables, images, date picker
# egui-notify = "0.17"     # Toast notifications
# rfd = "0.15"             # Native OS file dialogs

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
env_logger = "0.11"
```

### Feature flags (eframe)

| Feature | Default | Purpose |
|---------|---------|---------|
| `wgpu` | ✅ (0.34+) | wgpu rendering backend (WebGPU + WebGL2) |
| `glow` | ❌ | OpenGL rendering via glow + glutin |
| `default_fonts` | ✅ | Bundle default fonts |
| `persistence` | ❌ | Save/restore app state to disk |
| `accesskit` | ✅ | Screen reader APIs |
| `wayland` | ✅ | Wayland on Linux |
| `x11` | ✅ | X11 on Linux |

---

## Minimal app template (0.34 style — preferred)

```rust
use eframe::egui;

fn main() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0]),
        ..Default::default()
    };
    eframe::run_native("My App", options, Box::new(|cc| Ok(Box::new(MyApp::new(cc)))))
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
struct MyApp {
    name: String,
    age: u32,
}

impl MyApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Restore state:
        if let Some(storage) = cc.storage {
            return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
        }
        Self::default()
    }
}

impl eframe::App for MyApp {
    // NEW 0.34 preferred entry point:
    fn ui(&mut self, ui: &mut egui::Ui) {
        egui::TopBottomPanel::top("menu").show(ui, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("My App");
            ui.horizontal(|ui| {
                ui.label("Name:");
                ui.text_edit_singleline(&mut self.name);
            });
            ui.add(egui::Slider::new(&mut self.age, 0..=120).text("age"));
            if ui.button("Increment").clicked() {
                self.age += 1;
            }
            ui.label(format!("Hello '{}', age {}", self.name, self.age));
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
}
```

### Backward-compatible style (works in 0.33 and 0.34)

```rust
impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Hello");
        });
    }
}
```

---

## 0.34 breaking changes from 0.33

| Change | Before (0.33) | After (0.34) |
|--------|---------------|--------------|
| App entry point | `fn update(&mut self, ctx: &Context, frame: &mut Frame)` | `fn ui(&mut self, ui: &mut egui::Ui)` |
| Context run | `ctx.run(raw_input, \|ctx\| { ... })` | `ctx.run_ui(raw_input, \|ui\| { ... })` |
| Style access | `ctx.style()` | `ctx.global_style()` |
| Panels | `Panel::show(ctx, \|ui\| ...)` | `Panel::show(ui, \|ui\| ...)` (on Context deprecated) |
| Viewports | callback receives `Context` | callback receives `&mut Ui` |
| Ui→Context | `ui.ctx().input(...)` | `ui.input(...)` (Ui derefs to Context) |
| Fonts | `ab_glyph` | `skrifa` + `vello_cpu` (sharper with hinting) |
| Default renderer | `glow` | `wgpu` |
| Rounding type | `Rounding` | `CornerRadius` (renamed) |

---

## eframe::App trait (full)

```rust
pub trait App {
    fn update(&mut self, ctx: &Context, frame: &mut Frame) {} // DEPRECATED 0.34
    fn ui(&mut self, ui: &mut egui::Ui) {}                    // NEW 0.34 preferred
    fn save(&mut self, _storage: &mut dyn Storage) {}          // persistence feature
    fn on_exit(&mut self, _gl: Option<&glow::Context>) {}      // shutdown hook
    fn auto_save_interval(&self) -> Duration { Duration::from_secs(30) }
    fn clear_color(&self, _visuals: &Visuals) -> [f32; 4] { .. }
    fn persist_egui_memory(&self) -> bool { true }
    fn raw_input_hook(&mut self, _ctx: &Context, _raw_input: &mut RawInput) {}
}
```

### NativeOptions key fields

```rust
eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default()
        .with_inner_size([800.0, 600.0])
        .with_min_inner_size([300.0, 200.0])
        .with_icon(icon_data),
    vsync: true,
    multisampling: 0,
    renderer: eframe::Renderer::Wgpu,  // or Renderer::Glow
    persist_window: true,
    centered: true,
    ..Default::default()
}
```

---

## Core architecture

### Immediate mode paradigm

Every frame: Input → UI code runs → Shapes generated → Tessellated → Rendered. No retained widget tree. Widgets are functions that return `Response`.

```rust
// The "immediate mode" pattern: show and handle in one expression
if ui.button("Save").clicked() {
    save_file();
}
// Slider modifies value directly through mutable reference
ui.add(egui::Slider::new(&mut self.value, 0.0..=100.0));
```

### Frame lifecycle

1. Backend gathers `RawInput` (mouse, keyboard, screen size, time)
2. `begin_pass(raw_input)` — processes input
3. UI code adds panels/windows/widgets via `&mut Ui`
4. `end_pass()` → `FullOutput` (shapes, textures_delta, platform_output)
5. `ctx.tessellate(shapes)` → `Vec<ClippedPrimitive>` (triangles)
6. Backend renders triangles via glow/wgpu
7. If `request_repaint()` called or interaction occurred → schedule next frame

Multiple passes per frame possible via `request_discard()` (max 2 default).

### Context key methods

```rust
// Repaint control:
ctx.request_repaint();                          // repaint ASAP
ctx.request_repaint_after(Duration::from_secs(1));  // periodic
ctx.request_repaint_of(viewport_id);            // specific viewport

// Style:
ctx.global_style() -> Arc<Style>;               // renamed from style() in 0.34
ctx.set_style(style);
ctx.set_visuals(Visuals::dark());
ctx.set_theme(egui::Theme::Light);
ctx.set_fonts(FontDefinitions { .. });
ctx.all_styles_mut(|style| { .. });

// Input (read-only closure to avoid deadlocks):
ctx.input(|i| i.key_pressed(Key::Enter));
ctx.input(|i| i.modifiers.command);
ctx.input_mut(|i| i.consume_shortcut(&shortcut));

// Memory:
ctx.memory(|mem| mem.data.get_temp::<T>(id));
ctx.memory_mut(|mem| mem.data.insert_temp(id, value));

// Animation:
ctx.animate_bool(id, is_open) -> f32;           // 0.0..=1.0 smooth
ctx.animate_value_with_time(id, target, duration) -> f32;
```

### Ui — the primary widget interface

In 0.34, `Ui` implements `Deref<Target = Context>`, so all Context methods work directly on Ui.

```rust
// Obtained via panel/window closures:
egui::CentralPanel::default().show(ui, |ui| {
    ui.label("text");              // add widgets
    ui.available_width();          // query space
    ui.painter();                  // get Painter for custom drawing
    ui.input(|i| i.time);         // access input (via Deref to Context)
    ui.request_repaint();          // (via Deref to Context)
    ui.ctx();                      // explicit Context access still works
});
```

---

## All major widgets

### Widget trait

```rust
pub trait Widget {
    fn ui(self, ui: &mut Ui) -> Response;
}
// Also implemented for closures: impl<F: FnOnce(&mut Ui) -> Response> Widget for F
// Add any widget: ui.add(my_widget)
```

### Button

```rust
// Shortcuts:
if ui.button("Click me").clicked() { }
ui.small_button("x");
ui.add_enabled(is_enabled, egui::Button::new("Conditional"));

// Full builder:
ui.add(egui::Button::new("Styled")
    .fill(Color32::from_rgb(0, 100, 200))
    .stroke(Stroke::new(1.0, Color32::WHITE))
    .corner_radius(8)
    .min_size(egui::vec2(100.0, 40.0))
    .sense(Sense::click_and_drag())
    .shortcut_text("Ctrl+S")          // right-aligned hint text
    .selected(is_active)               // toggle style
);

// Image button:
ui.add(egui::Button::image(egui::include_image!("icon.png")));
ui.add(egui::Button::image_and_text(egui::include_image!("icon.png"), "Label"));
```

### Label

```rust
ui.label("Plain text");
ui.heading("Section Title");
ui.strong("Bold");
ui.weak("Dimmed");
ui.monospace("code_text");
ui.code("inline code");                      // monospace + background
ui.small("Fine print");
ui.colored_label(Color32::RED, "Red text");
ui.link("Clickable text");                   // returns Response

// RichText for full control:
ui.label(egui::RichText::new("Fancy")
    .size(24.0)
    .color(Color32::GOLD)
    .strong()
    .underline()
    .italics()
    .background_color(Color32::from_black_alpha(20))
);

// Label builder:
ui.add(egui::Label::new("Wrapped text").wrap());
ui.add(egui::Label::new("Selectable").selectable(true));
```

### TextEdit

```rust
// Shortcuts:
ui.text_edit_singleline(&mut my_string);
ui.text_edit_multiline(&mut my_string);
ui.code_editor(&mut my_code);                // monospace + no wrap

// Full builder:
let response = ui.add(egui::TextEdit::singleline(&mut self.search)
    .hint_text("Search…")
    .desired_width(200.0)
    .char_limit(100)
    .font(egui::TextStyle::Monospace)
    .password(true)
    .lock_focus(true)           // keep focus on Enter
);
if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
    perform_search(&self.search);
}
if response.changed() { /* text modified this frame */ }

// Multiline:
ui.add(egui::TextEdit::multiline(&mut self.notes)
    .desired_rows(6)
    .desired_width(f32::INFINITY)   // fill available width
    .code_editor()
    .margin(egui::Margin::same(8))
);
```

### Slider

```rust
ui.add(egui::Slider::new(&mut self.value, 0.0..=100.0)
    .text("Volume")
    .suffix("%")
    .logarithmic(true)
    .step_by(1.0)
    .fixed_decimals(1)
    .trailing_fill(true)        // colored fill behind handle
);

// Vertical slider:
ui.add(egui::Slider::new(&mut val, 0.0..=1.0).vertical());

// Integer slider:
ui.add(egui::Slider::new(&mut my_i32, 0..=120).integer());
```

### DragValue

```rust
ui.add(egui::DragValue::new(&mut self.x)
    .speed(0.1)                    // values per pixel dragged
    .range(0.0..=100.0)
    .prefix("x: ")
    .suffix(" px")
    .fixed_decimals(2)
);
// Sized:
ui.add_sized([60.0, 20.0], egui::DragValue::new(&mut self.value));
```

### Checkbox and RadioButton

```rust
// Checkbox:
ui.checkbox(&mut self.enabled, "Enable feature");

// Radio buttons:
ui.radio_value(&mut self.mode, Mode::Edit, "Edit");
ui.radio_value(&mut self.mode, Mode::View, "View");
ui.radio_value(&mut self.mode, Mode::Preview, "Preview");

// Manual radio:
if ui.radio(self.mode == Mode::Edit, "Edit").clicked() {
    self.mode = Mode::Edit;
}
```

### ComboBox (dropdown)

```rust
egui::ComboBox::from_label("Select one")
    .selected_text(format!("{:?}", self.selected))
    .show_ui(ui, |ui| {
        ui.selectable_value(&mut self.selected, Choice::A, "Choice A");
        ui.selectable_value(&mut self.selected, Choice::B, "Choice B");
        ui.selectable_value(&mut self.selected, Choice::C, "Choice C");
    });

// With custom width:
egui::ComboBox::from_id_salt("combo2")
    .selected_text(&self.items[self.idx])
    .width(200.0)
    .show_ui(ui, |ui| {
        for (i, item) in self.items.iter().enumerate() {
            ui.selectable_value(&mut self.idx, i, item);
        }
    });
```

### SelectableLabel / toggle

```rust
ui.selectable_label(self.selected == 0, "Option A");
ui.selectable_value(&mut self.selected, 0, "Option A");
ui.toggle_value(&mut self.is_on, "Toggle me");
```

### CollapsingHeader

```rust
ui.collapsing("Details", |ui| {
    ui.label("Hidden content");
});

egui::CollapsingHeader::new("Advanced")
    .default_open(true)
    .id_salt("advanced_section")
    .show(ui, |ui| {
        ui.label("Content");
    });
```

### ScrollArea

```rust
egui::ScrollArea::vertical().show(ui, |ui| {
    for i in 0..1000 {
        ui.label(format!("Item {i}"));
    }
});

egui::ScrollArea::both()
    .max_height(300.0)
    .auto_shrink(false)
    .show(ui, |ui| { /* content */ });

// Virtual scrolling for large lists:
let row_height = ui.text_style_height(&egui::TextStyle::Body);
egui::ScrollArea::vertical().show_rows(ui, row_height, total_rows, |ui, row_range| {
    for row in row_range {
        ui.label(format!("Row {row}"));
    }
});
```

### ProgressBar

```rust
ui.add(egui::ProgressBar::new(0.7)
    .show_percentage()
    .animate(true)
    .desired_width(200.0)
    .fill(Color32::GREEN)
);
```

### Image

```rust
// From embedded bytes (compile-time):
ui.image(egui::include_image!("assets/logo.png"));

// From URL (requires image loaders):
ui.image("https://example.com/image.png");

// From TextureHandle:
ui.image((texture.id(), texture.size_vec2()));

// Builder:
ui.add(egui::Image::new(egui::include_image!("photo.jpg"))
    .max_width(300.0)
    .corner_radius(10)
    .tint(Color32::from_white_alpha(200))
);
```

### Other widgets

```rust
ui.separator();                                     // horizontal line
ui.add_space(10.0);                                 // spacing
ui.spinner();                                       // loading spinner
ui.add(egui::Spinner::new().size(32.0));
ui.hyperlink("https://github.com/emilk/egui");
ui.hyperlink_to("egui", "https://github.com/emilk/egui");
ui.color_edit_button_srgba(&mut self.color);        // color picker popup
ui.drag_angle(&mut self.angle);                     // angle drag widget
```

### Ui convenience method summary

| Shortcut | Widget |
|----------|--------|
| `ui.label(text)` | `Label::new(text)` |
| `ui.heading(text)` | Label with heading style |
| `ui.button(text)` | `Button::new(text)` |
| `ui.small_button(text)` | `Button::new(text).small()` |
| `ui.checkbox(&mut bool, text)` | `Checkbox` |
| `ui.radio_value(&mut val, sel, text)` | `RadioButton` + assignment |
| `ui.selectable_label(selected, text)` | Selectable button |
| `ui.text_edit_singleline(&mut s)` | `TextEdit::singleline` |
| `ui.text_edit_multiline(&mut s)` | `TextEdit::multiline` |
| `ui.code_editor(&mut s)` | `TextEdit::multiline.code_editor()` |
| `ui.separator()` | `Separator` |
| `ui.spinner()` | `Spinner` |
| `ui.image(source)` | `Image::new(source)` |
| `ui.add(widget)` | Any `impl Widget` |
| `ui.add_sized([w,h], widget)` | Widget with forced size |
| `ui.add_enabled(bool, widget)` | Conditionally disabled |
| `ui.add_visible(bool, widget)` | Conditionally hidden |

---

## Layout system

### Panel ordering rule

**CentralPanel MUST be added LAST**, after all SidePanel and TopBottomPanel instances. First panel added = outermost.

```rust
fn ui(&mut self, ui: &mut egui::Ui) {
    egui::TopBottomPanel::top("top").show(ui, |ui| {
        egui::menu::bar(ui, |ui| { /* menu */ });
    });
    egui::SidePanel::left("left").show(ui, |ui| { /* sidebar */ });
    egui::TopBottomPanel::bottom("status").show(ui, |ui| { /* status bar */ });
    egui::CentralPanel::default().show(ui, |ui| { /* main content — LAST */ });
}
```

### SidePanel

```rust
egui::SidePanel::left("nav")
    .resizable(true)
    .default_width(200.0)
    .width_range(100.0..=400.0)
    .show(ui, |ui| { /* content */ });

egui::SidePanel::right("properties")
    .exact_width(250.0)
    .show(ui, |ui| { /* content */ });

// Animated show/hide:
egui::SidePanel::left("panel")
    .show_animated(ui, self.show_panel, |ui| { /* content */ });
```

### TopBottomPanel

```rust
egui::TopBottomPanel::top("toolbar")
    .exact_height(40.0)
    .show(ui, |ui| { /* toolbar */ });

egui::TopBottomPanel::bottom("status")
    .resizable(false)
    .show(ui, |ui| { ui.label("Ready"); });
```

### Window

```rust
egui::Window::new("Settings")
    .open(&mut self.show_settings)   // close button; controls visibility
    .default_size([400.0, 300.0])
    .resizable(true)
    .collapsible(true)
    .scroll([false, true])
    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
    .show(ui.ctx(), |ui| {           // Windows still shown via ctx or top-level ui
        ui.label("Window content");
    });

// Fixed position, no decorations:
egui::Window::new("HUD")
    .title_bar(false)
    .fixed_pos([10.0, 10.0])
    .fixed_size([200.0, 50.0])
    .show(ui.ctx(), |ui| { /* overlay */ });
```

### Layout methods on Ui

```rust
// Horizontal row:
ui.horizontal(|ui| {
    ui.label("Name:");
    ui.text_edit_singleline(&mut name);
    if ui.button("OK").clicked() { }
});

// Horizontal with wrapping:
ui.horizontal_wrapped(|ui| {
    for tag in &tags {
        ui.button(tag);
    }
});

// Vertical (default):
ui.vertical(|ui| { });
ui.vertical_centered(|ui| { });
ui.vertical_centered_justified(|ui| { });

// Custom layout:
ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
    ui.button("Right-aligned");
});

// Columns (equal width):
ui.columns(3, |cols| {
    cols[0].label("Column 1");
    cols[1].label("Column 2");
    cols[2].label("Column 3");
});

// Group (visual frame):
ui.group(|ui| {
    ui.label("Grouped content");
});

// Indent:
ui.indent("indent1", |ui| {
    ui.label("Indented");
});

// Scope (temporary style changes):
ui.scope(|ui| {
    ui.visuals_mut().override_text_color = Some(Color32::RED);
    ui.label("Red text only here");
});
```

### Grid

```rust
egui::Grid::new("my_grid")
    .num_columns(2)
    .striped(true)
    .spacing([40.0, 4.0])
    .show(ui, |ui| {
        ui.label("Name:");
        ui.text_edit_singleline(&mut self.name);
        ui.end_row();

        ui.label("Age:");
        ui.add(egui::DragValue::new(&mut self.age));
        ui.end_row();

        ui.label("Active:");
        ui.checkbox(&mut self.active, "");
        ui.end_row();
    });
```

### Layout struct

```rust
Layout::left_to_right(Align::Center)     // horizontal L→R
Layout::right_to_left(Align::Center)     // horizontal R→L
Layout::top_down(Align::LEFT)            // vertical top→bottom
Layout::top_down_justified(Align::LEFT)  // vertical, widgets fill width
Layout::bottom_up(Align::LEFT)           // vertical bottom→up
Layout::centered_and_justified(Direction::TopDown)  // single centered widget

// Builder modifiers:
.with_main_wrap(true)
.with_cross_align(Align::Center)
.with_cross_justify(true)
```

### Area (floating content)

```rust
egui::Area::new(egui::Id::new("floating"))
    .fixed_pos(egui::pos2(100.0, 100.0))
    .show(ui.ctx(), |ui| {
        ui.label("Floating content");
    });
```

---

## Styling and theming

### Theme switching

```rust
ctx.set_theme(egui::Theme::Dark);
ctx.set_theme(egui::Theme::Light);

// Toggle:
if ui.button("🌙/☀").clicked() {
    let theme = if ui.visuals().dark_mode { egui::Theme::Light } else { egui::Theme::Dark };
    ui.ctx().set_theme(theme);
}

// Built-in toggle widget:
egui::Theme::from_dark_mode(ui.visuals().dark_mode).small_toggle_button(ui);
```

### Style struct (key fields)

```rust
pub struct Style {
    pub text_styles: BTreeMap<TextStyle, FontId>,
    pub spacing: Spacing,
    pub interaction: Interaction,
    pub visuals: Visuals,
    pub animation_time: f32,
    pub wrap_mode: Option<TextWrapMode>,
    ..
}

// Modify globally:
ctx.all_styles_mut(|style| {
    style.spacing.item_spacing = egui::vec2(10.0, 5.0);
    style.visuals.window_corner_radius = CornerRadius::same(12);
});

// Modify per-theme:
ctx.set_style_of(egui::Theme::Dark, my_dark_style);

// Modify locally (scoped):
ui.style_mut().spacing.item_spacing = egui::vec2(4.0, 2.0);
```

### Visuals (key fields)

```rust
pub struct Visuals {
    pub dark_mode: bool,
    pub override_text_color: Option<Color32>,
    pub widgets: Widgets,                      // per-state widget styles
    pub selection: Selection,                  // selection highlight
    pub hyperlink_color: Color32,
    pub faint_bg_color: Color32,
    pub extreme_bg_color: Color32,
    pub window_fill: Color32,
    pub window_stroke: Stroke,
    pub window_corner_radius: CornerRadius,
    pub window_shadow: Shadow,
    pub panel_fill: Color32,
    pub button_frame: bool,
    pub slider_trailing_fill: bool,
    pub striped: bool,
    pub indent_has_left_vline: bool,
    pub disabled_alpha: f32,
    ..
}

// Widget visual states:
pub struct Widgets {
    pub noninteractive: WidgetVisuals,  // labels, separators
    pub inactive: WidgetVisuals,        // interactive but not hovered
    pub hovered: WidgetVisuals,         // mouse over
    pub active: WidgetVisuals,          // being clicked/dragged
    pub open: WidgetVisuals,            // open (dropdown)
}
pub struct WidgetVisuals {
    pub bg_fill: Color32,
    pub weak_bg_fill: Color32,
    pub bg_stroke: Stroke,
    pub fg_stroke: Stroke,
    pub corner_radius: CornerRadius,
    pub expansion: f32,
}
```

### Spacing (key fields)

```rust
pub struct Spacing {
    pub item_spacing: Vec2,        // between widgets [8.0, 3.0]
    pub window_margin: Margin,
    pub button_padding: Vec2,
    pub indent: f32,               // collapsing indent
    pub interact_size: Vec2,       // min clickable size [40.0, 18.0]
    pub slider_width: f32,
    pub combo_width: f32,
    pub text_edit_width: f32,
    pub icon_width: f32,
    pub icon_spacing: f32,
    pub tooltip_width: f32,
    pub menu_width: f32,
    pub combo_height: f32,
    pub scroll: ScrollStyle,
    ..
}
```

### RichText

```rust
RichText::new("text")
    .size(20.0)                          // font size points
    .family(FontFamily::Monospace)
    .color(Color32::GOLD)
    .background_color(Color32::from_black_alpha(20))
    .strong()                            // bold
    .weak()                              // dimmed
    .italics()
    .underline()
    .strikethrough()
    .small()
    .heading()
    .monospace()
    .code()                              // monospace + bg
    .small_raised()                      // superscript
    .extra_letter_spacing(2.0)
```

### Color32

```rust
// Named constants:
Color32::BLACK, Color32::WHITE, Color32::RED, Color32::GREEN, Color32::BLUE,
Color32::YELLOW, Color32::TRANSPARENT, Color32::GOLD, Color32::GRAY,
Color32::DARK_GRAY, Color32::LIGHT_GRAY, Color32::BROWN, Color32::CYAN,
Color32::PURPLE, Color32::MAGENTA, Color32::ORANGE

// Constructors:
Color32::from_rgb(255, 128, 0)
Color32::from_rgba_unmultiplied(255, 128, 0, 200)
Color32::from_gray(128)
Color32::from_black_alpha(128)
Color32::from_white_alpha(200)
Color32::from_hex("#ff8800").unwrap()

// Operations:
color.gamma_multiply(0.5)     // fade
color.linear_multiply(0.5)
color.to_opaque()
```

### Stroke, CornerRadius, Shadow, Frame

```rust
Stroke::NONE
Stroke::new(2.0, Color32::WHITE)

CornerRadius::ZERO
CornerRadius::same(8)

Shadow::NONE
Shadow::small_dark()
Shadow::big_dark()

// Frame (container background):
Frame::NONE
Frame::group(ui.style())
Frame::window(ui.style())
Frame::canvas(ui.style())
Frame::popup(ui.style())

Frame {
    inner_margin: Margin::same(8),
    fill: Color32::from_gray(30),
    stroke: Stroke::new(1.0, Color32::GRAY),
    corner_radius: CornerRadius::same(6),
    outer_margin: Margin::ZERO,
    shadow: Shadow::NONE,
}
.show(ui, |ui| { ui.label("Framed"); });
```

### Font configuration

```rust
// Change text sizes:
let text_styles: BTreeMap<TextStyle, FontId> = [
    (TextStyle::Small,     FontId::new(10.0, FontFamily::Proportional)),
    (TextStyle::Body,      FontId::new(16.0, FontFamily::Proportional)),
    (TextStyle::Monospace, FontId::new(14.0, FontFamily::Monospace)),
    (TextStyle::Button,    FontId::new(14.0, FontFamily::Proportional)),
    (TextStyle::Heading,   FontId::new(24.0, FontFamily::Proportional)),
].into();
ctx.all_styles_mut(move |style| style.text_styles = text_styles.clone());

// Add custom font:
let mut fonts = FontDefinitions::default();
fonts.font_data.insert("my_font".to_owned(),
    egui::FontData::from_static(include_bytes!("../fonts/MyFont.ttf")));
fonts.families.entry(FontFamily::Proportional).or_default()
    .insert(0, "my_font".to_owned());
ctx.set_fonts(fonts);
```

---

## Input handling

### Response struct (returned by every widget)

```rust
let r = ui.button("Test");

// Clicks:
r.clicked()                      // primary click this frame
r.secondary_clicked()            // right-click
r.middle_clicked()
r.double_clicked()
r.triple_clicked()
r.clicked_by(PointerButton::Primary)
r.clicked_elsewhere()            // click outside this widget

// Hover:
r.hovered()                      // pointer over (false when dragging other)
r.contains_pointer()             // pointer in rect (always, even disabled)
r.hover_pos() -> Option<Pos2>

// Drag:
r.dragged()
r.drag_started()
r.drag_stopped()
r.drag_delta() -> Vec2           // frame delta
r.total_drag_delta() -> Vec2     // total since drag start
r.dragged_by(PointerButton::Primary)

// Focus:
r.has_focus()
r.gained_focus()
r.lost_focus()
r.request_focus()
r.surrender_focus()

// State:
r.changed()                      // value changed (slider, text, checkbox)
r.enabled()

// Decorators (chainable):
r.on_hover_text("Tooltip text")
r.on_hover_ui(|ui| { ui.label("Rich tooltip"); })
r.on_hover_cursor(egui::CursorIcon::PointingHand)
r.on_disabled_hover_text("Disabled because…")
r.context_menu(|ui| { ui.button("Copy"); })
r.highlight()

// Combining responses:
let combined = r1 | r2;         // union of two responses

// Drag and Drop:
r.dnd_set_drag_payload(MyData { .. });
r.dnd_hover_payload::<MyData>()  -> Option<Arc<MyData>>
r.dnd_release_payload::<MyData>() -> Option<Arc<MyData>>

// Fields:
r.rect                           // widget rect on screen
r.id                             // widget Id
```

### Sense

```rust
Sense::hover()              // just hover detection
Sense::click()              // click + focusable
Sense::drag()               // drag + focusable
Sense::click_and_drag()     // both (adds latency to distinguish)
Sense::focusable_noninteractive()
```

### Keyboard input

```rust
// Key press check:
ui.input(|i| i.key_pressed(egui::Key::Enter))
ui.input(|i| i.key_pressed(egui::Key::Escape))

// Modifiers:
ui.input(|i| i.modifiers.command)  // Cmd on Mac, Ctrl on Win/Linux
ui.input(|i| i.modifiers.shift)
ui.input(|i| i.modifiers.alt)

// Consume shortcut (prevents double-handling):
let shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);
if ui.input_mut(|i| i.consume_shortcut(&shortcut)) {
    save();
}

// Format for display:
ctx.format_shortcut(&shortcut)  // "⌘S" on Mac, "Ctrl+S" on Windows

// Time & pointer:
ui.input(|i| i.time)                    // seconds since start
ui.input(|i| i.pointer.delta())         // pointer movement
ui.input(|i| i.raw.dropped_files.clone()) // dropped files

// Modifiers struct:
Modifiers::NONE, Modifiers::COMMAND, Modifiers::CTRL, Modifiers::SHIFT, Modifiers::ALT
```

---

## State management

### Id system

```rust
Id::NULL                              // valid but avoid reusing
Id::new("my_unique_string")          // from any hashable
Id::new(("widget", index))           // compound
id.with("child")                      // hierarchical

ui.id()                               // current Ui's Id
ui.make_persistent_id("something")    // scoped to current Ui
ui.push_id(i, |ui| { .. });          // unique scope per iteration
```

### Memory and data persistence

```rust
// Temporary data (not saved to disk):
ui.data_mut(|d| d.insert_temp(id, my_value));
let val: Option<T> = ui.data(|d| d.get_temp::<T>(id));

// Persistent data (saved with "persistence" feature):
ui.data_mut(|d| d.insert_persisted(id, my_value));
let val = ui.data(|d| d.get_persisted::<T>(id));

// Via Context:
ctx.memory_mut(|mem| mem.data.insert_temp(id, value));
ctx.memory(|mem| mem.data.get_temp::<T>(id));

// eframe persistence (save/restore entire app state):
fn save(&mut self, storage: &mut dyn eframe::Storage) {
    eframe::set_value(storage, eframe::APP_KEY, self);
}
// In new():
if let Some(storage) = cc.storage {
    return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
}
```

**Key rules**: Each `memory()` read clones data — keep values small or use `Arc`. Never nest context lock closures (`ctx.input(|i| { ctx.memory(|m| ..) })` deadlocks).

---

## Images and textures

### Loading textures (do once, not every frame)

```rust
struct MyApp {
    texture: Option<egui::TextureHandle>,
}

fn ui(&mut self, ui: &mut egui::Ui) {
    let texture: &egui::TextureHandle = self.texture.get_or_insert_with(|| {
        // Load ONCE:
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [width, height],
            &rgba_bytes,
        );
        ui.ctx().load_texture("my-image", image, Default::default())
    });
    ui.image((texture.id(), texture.size_vec2()));
}
```

### ColorImage constructors

```rust
ColorImage::new([w, h], Color32::BLACK)
ColorImage::from_rgba_unmultiplied([w, h], &bytes)
ColorImage::from_rgba_premultiplied([w, h], &bytes)
ColorImage::from_gray([w, h], &gray_bytes)
ColorImage::example()  // test pattern
```

### TextureOptions

```rust
TextureOptions {
    magnification: TextureFilter::Linear,  // or Nearest
    minification: TextureFilter::Linear,
    wrap_mode: TextureWrapMode::ClampToEdge,  // or Repeat, MirroredRepeat
}
// Default: Linear filtering, ClampToEdge
```

### Dynamic pixel updates

```rust
let mut img = ColorImage::new([w, h], Color32::BLACK);
img.pixels[y * w + x] = Color32::from_rgb(255, 0, 0);
self.texture.as_mut().unwrap().set(img, Default::default());
```

### include_image! macro

```rust
// Embed image at compile time — returns ImageSource:
ui.image(egui::include_image!("assets/logo.png"));
// Requires image feature in egui_extras or appropriate loader
```

---

## Custom painting

### Painter API

```rust
// Get painter from Ui:
let (response, painter) = ui.allocate_painter(
    egui::Vec2::new(300.0, 200.0),
    Sense::hover(),
);
let rect = response.rect;

// Lines:
painter.line_segment([pos1, pos2], Stroke::new(2.0, Color32::WHITE));
painter.line(points_vec, Stroke::new(1.0, Color32::RED));
painter.hline(x_range, y, stroke);
painter.vline(x, y_range, stroke);

// Circles:
painter.circle_filled(center, radius, Color32::BLUE);
painter.circle_stroke(center, radius, Stroke::new(1.0, Color32::WHITE));
painter.circle(center, radius, Color32::BLUE, Stroke::new(1.0, Color32::WHITE));

// Rectangles:
painter.rect_filled(rect, corner_radius, Color32::from_gray(40));
painter.rect_stroke(rect, corner_radius, Stroke::new(1.0, Color32::WHITE), StrokeKind::Outside);

// Text:
painter.text(pos, Align2::CENTER_CENTER, "Hello",
    FontId::proportional(16.0), Color32::WHITE);

// Arrow:
painter.arrow(origin, direction_vec, Stroke::new(2.0, Color32::GREEN));

// Any shape:
painter.add(Shape::circle_filled(pos, 5.0, Color32::RED));
```

### Shape enum variants

```rust
Shape::Noop
Shape::Vec(Vec<Shape>)
Shape::Circle(CircleShape)
Shape::Ellipse(EllipseShape)
Shape::LineSegment { points: [Pos2; 2], stroke: Stroke }
Shape::Path(PathShape)
Shape::Rect(RectShape)
Shape::Text(TextShape)
Shape::Mesh(Arc<Mesh>)
Shape::QuadraticBezier(QuadraticBezierShape)
Shape::CubicBezier(CubicBezierShape)
Shape::Callback(PaintCallback)    // for custom GPU rendering
```

### Local coordinate system

```rust
let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());
let to_screen = emath::RectTransform::from_to(
    Rect::from_min_size(Pos2::ZERO, response.rect.size()),
    response.rect,
);
// Convert local → screen:
let screen_pos = to_screen.transform_pos(egui::pos2(50.0, 50.0));
```

### Custom widget pattern

```rust
struct ToggleSwitch<'a> {
    on: &'a mut bool,
}

impl<'a> Widget for ToggleSwitch<'a> {
    fn ui(self, ui: &mut Ui) -> Response {
        let desired_size = egui::vec2(36.0, 20.0);
        let (rect, mut response) = ui.allocate_exact_size(desired_size, Sense::click());

        if response.clicked() {
            *self.on = !*self.on;
            response.mark_changed();
        }

        if ui.is_rect_visible(rect) {
            let how_on = ui.ctx().animate_bool(response.id, *self.on);
            let visuals = ui.style().interact(&response);
            let rect = rect.expand(visuals.expansion);
            let radius = 0.5 * rect.height();
            // Background:
            painter.rect_filled(rect, radius, visuals.bg_fill);
            // Circle handle:
            let circle_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
            let center = egui::pos2(circle_x, rect.center().y);
            painter.circle_filled(center, 0.75 * radius, visuals.fg_stroke.color);
        }
        response
    }
}
// Usage: ui.add(ToggleSwitch { on: &mut self.enabled });
```

### Closure widget shortcut

```rust
fn my_widget(value: &mut f32) -> impl Widget + '_ {
    move |ui: &mut Ui| -> Response {
        ui.horizontal(|ui| {
            ui.label("Value:");
            ui.add(egui::Slider::new(value, 0.0..=1.0));
        }).response
    }
}
ui.add(my_widget(&mut self.val));
```

---

## Common patterns and idioms

### Modal dialog (built-in)

```rust
if self.show_modal {
    egui::Modal::new(egui::Id::new("confirm_modal")).show(ui.ctx(), |ui| {
        ui.heading("Confirm");
        ui.label("Are you sure?");
        ui.horizontal(|ui| {
            if ui.button("Yes").clicked() { self.confirmed = true; self.show_modal = false; }
            if ui.button("Cancel").clicked() { self.show_modal = false; }
        });
    });
}
```

### Context menu (right-click)

```rust
let response = ui.label("Right-click me");
response.context_menu(|ui| {
    if ui.button("Copy").clicked() { ui.close_menu(); }
    if ui.button("Paste").clicked() { ui.close_menu(); }
    ui.separator();
    ui.menu_button("More…", |ui| {
        if ui.button("Sub-option").clicked() { ui.close_menu(); }
    });
});
```

### Menu bar

```rust
egui::TopBottomPanel::top("menu").show(ui, |ui| {
    egui::menu::bar(ui, |ui| {
        ui.menu_button("File", |ui| {
            let save_shortcut = egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
            if ui.add(egui::Button::new("Save")
                .shortcut_text(ui.ctx().format_shortcut(&save_shortcut)))
                .clicked() {
                save();
                ui.close_menu();
            }
            if ui.button("Quit").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ui.menu_button("Edit", |ui| {
            ui.button("Undo");
            ui.button("Redo");
        });
    });
});
```

### Background thread communication

```rust
use std::sync::mpsc;

struct MyApp {
    tx: mpsc::Sender<String>,
    rx: mpsc::Receiver<String>,
    result: Option<String>,
}

// Spawn work:
if ui.button("Fetch").clicked() {
    let tx = self.tx.clone();
    let ctx = ui.ctx().clone();  // Context is Arc-based, Send+Sync
    std::thread::spawn(move || {
        let data = expensive_work();
        tx.send(data).unwrap();
        ctx.request_repaint();  // wake up UI thread
    });
}

// Poll (non-blocking):
if let Ok(data) = self.rx.try_recv() {
    self.result = Some(data);
}
```

### Drag and drop

```rust
// Drag source:
let response = ui.button("Drag me");
if response.dragged() {
    response.dnd_set_drag_payload(MyPayload { id: 42 });
}

// Drop target:
let target = ui.button("Drop here");
if let Some(payload) = target.dnd_release_payload::<MyPayload>() {
    handle_drop(payload.id);
}
```

### File drop handling

```rust
ctx.input(|i| {
    for file in &i.raw.dropped_files {
        if let Some(path) = &file.path {
            self.open_file(path);
        }
    }
});
```

### Keyboard shortcut handling

```rust
// Use consume_shortcut to prevent double-handling (check specific before general):
let save_as = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
let save = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);

if ui.input_mut(|i| i.consume_shortcut(&save_as)) {
    save_as_dialog();
} else if ui.input_mut(|i| i.consume_shortcut(&save)) {
    save_file();
}
```

### Viewport (multi-window, native only)

```rust
if self.show_about {
    let show = Arc::clone(&self.show_about_arc);
    ui.ctx().show_viewport_deferred(
        egui::ViewportId::from_hash_of("about"),
        egui::ViewportBuilder::default()
            .with_title("About")
            .with_inner_size([300.0, 200.0]),
        move |ctx, _class| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("About");
            });
            if ctx.input(|i| i.viewport().close_requested()) {
                show.store(false, Ordering::Relaxed);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        },
    );
}
```

### egui_plot (separate crate)

```rust
// Cargo.toml: egui_plot = "0.34"
use egui_plot::{Line, Plot, PlotPoints};

Plot::new("sine").view_aspect(2.0).show(ui, |plot_ui| {
    let points: PlotPoints = (0..1000).map(|i| {
        let x = i as f64 * 0.01;
        [x, x.sin()]
    }).collect();
    plot_ui.line(Line::new("sin", points));
});
```

---

## Performance tips

- **Use `request_repaint_after(Duration)`** for periodic updates (clocks, timers) instead of continuous `request_repaint()`
- **Never call `request_repaint()` every frame** unless running animations or games — wastes CPU
- **Cache expensive computations** between frames in your app struct
- **Load textures once** via `get_or_insert_with` pattern — never in the hot loop
- **Virtual scrolling**: Use `ScrollArea::show_rows()` for large lists instead of laying out all items
- **Don't `.await` in UI code** — it freezes the thread. Use channels + background threads
- **Keep `update()`/`ui()` fast** — target < 2ms. Offload heavy work to threads
- **Skip work when minimized**: `if ctx.input(|i| i.viewport().minimized.unwrap_or(false)) { return; }`
- **Don't nest context lock closures**: `ctx.input(|i| { ctx.memory(|m| ...) })` deadlocks

---

## Common pitfalls

**ID conflicts**: Two `Window::new("Settings")` with the same title share an ID and conflict. Fix: `.id(Id::new("unique"))`. In loops: `ui.push_id(i, |ui| { .. })`.

**Forgetting Response**: `ui.button("Save");` ignores the click. Always check: `if ui.button("Save").clicked() { .. }`.

**Panel ordering**: `CentralPanel` must come last. Adding it before `SidePanel` causes layout issues.

**Deadlocks**: Never nest context accessor closures: `ctx.input(|i| { ctx.memory(..) })`. Extract values first, then use them.

**Window visibility**: `show_viewport_*` must be called every frame the viewport should exist. Calling it only inside `clicked()` → window flashes for one frame.

**Texture every frame**: Calling `ctx.load_texture()` every frame instead of caching the `TextureHandle` is extremely expensive.

**TextEdit + Enter**: `lost_focus()` fires when Enter is pressed in singleline TextEdit. Combine with `key_pressed(Key::Enter)` to detect submission:
```rust
if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) { submit(); }
```

**cross-platform modifier**: Use `Modifiers::COMMAND` (maps to Cmd on Mac, Ctrl on Win/Linux) instead of `Modifiers::CTRL`.