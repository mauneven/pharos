//! The Pharos window: shows the reminders and edits the config file.
//!
//! The clocks run in the background process (`pharos`). This window keeps a
//! copy of them for the countdowns, read from the state file the background
//! process publishes, and tells it to `reload` after every save.

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use crate::theme::Theme;
use pharos_core::config::{self, Config, MAX_MINUTES, MIN_MINUTES, Reminder};
use pharos_core::scheduler::{self, Scheduler};
use pharos_core::{autostart, i18n, ipc, paths};

/// Widest the content column grows; past that it stays centered.
const COLUMN: f64 = 520.0;

const ICON_EDIT: &str = "\u{f03eb}"; // nf-md-pencil
const ICON_DONE: &str = "\u{f012c}"; // nf-md-check
const ICON_DELETE: &str = "\u{f01b4}"; // nf-md-delete
const ICON_ADD: &str = "\u{f0415}"; // nf-md-plus

pub struct Pharos {
    app: gtk::Application,
    config: RefCell<Config>,
    scheduler: RefCell<Scheduler>,
    editing: Cell<bool>,
    cursor: Cell<usize>,
    theme: RefCell<Theme>,
    provider: gtk::CssProvider,
    ui: RefCell<Option<Ui>>,
    save_source: RefCell<Option<glib::SourceId>>,
    reload_source: RefCell<Option<glib::SourceId>>,
    monitors: RefCell<Vec<gio::FileMonitor>>,
}

struct Ui {
    window: gtk::ApplicationWindow,
    root: gtk::Box,
    scroller: gtk::ScrolledWindow,
    columns: Vec<gtk::Widget>,
    list: gtk::Box,
    rows: Vec<Row>,
    master: gtk::Switch,
    edit_button: gtk::Button,
    edit_icon: gtk::Label,
    status: gtk::Label,
    hint: gtk::Label,
    edit_extras: gtk::Box,
    add_label: gtk::Label,
    autostart: gtk::CheckButton,
}

struct Row {
    id: u32,
    root: gtk::Box,
    check: gtk::CheckButton,
    scale: gtk::Scale,
    value: gtk::Label,
    countdown: gtk::Label,
    entry: Option<gtk::Entry>,
}

impl Pharos {
    pub fn new(app: &gtk::Application, config: Config) -> Rc<Self> {
        i18n::reload();
        let provider = gtk::CssProvider::new();
        if let Some(display) = gdk::Display::default() {
            #[allow(deprecated)]
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_USER,
            );
        }
        let mut scheduler = Scheduler::default();
        scheduler.restart_all(&config, SystemTime::now());
        if ipc::send("ping").is_err() {
            start_background();
        } else if let Some(deadlines) = ipc::read_state() {
            scheduler.set_deadlines(deadlines);
        }

        let this = Rc::new(Pharos {
            app: app.clone(),
            config: RefCell::new(config),
            scheduler: RefCell::new(scheduler),
            editing: Cell::new(false),
            cursor: Cell::new(0),
            theme: RefCell::new(Theme::load()),
            provider,
            ui: RefCell::new(None),
            save_source: RefCell::new(None),
            reload_source: RefCell::new(None),
            monitors: RefCell::new(Vec::new()),
        });
        this.apply_theme();
        this.watch_files();

        let tick = this.clone();
        glib::timeout_add_seconds_local(1, move || {
            tick.tick();
            glib::ControlFlow::Continue
        });
        this
    }

    // ------------------------------------------------------------ clock

    fn tick(self: &Rc<Self>) {
        if self.window_visible() {
            self.refresh_live();
        }
    }

    /// Takes the clocks the background process published.
    fn sync_clocks(&self) {
        if let Some(deadlines) = ipc::read_state() {
            self.scheduler.borrow_mut().set_deadlines(deadlines);
            self.refresh_live();
        }
    }

    /// The config file changed outside this window (the tray's on/off, or
    /// a hand edit).
    fn reload_config(self: &Rc<Self>) {
        if self.save_source.borrow().is_some() {
            return; // our own edit is about to land
        }
        let new = match Config::load_from(&paths::config_file()) {
            config::Loaded::FirstRun(cfg) | config::Loaded::Existing(cfg) => cfg,
        };
        if new == *self.config.borrow() {
            return;
        }
        let on = new.enabled;
        *self.config.borrow_mut() = new;
        self.show_master(on);
        self.rebuild();
        self.refresh_live();
    }

    fn restart_clock(&self, id: u32) {
        let cfg = self.config.borrow();
        if let Some(r) = cfg.reminders.iter().find(|r| r.id == id) {
            self.scheduler
                .borrow_mut()
                .restart(&cfg, r, SystemTime::now());
        }
    }

    // ------------------------------------------------------------ persistence

    fn schedule_save(self: &Rc<Self>) {
        if let Some(source) = self.save_source.borrow_mut().take() {
            source.remove();
        }
        let this = self.clone();
        let source = glib::timeout_add_local_once(Duration::from_millis(400), move || {
            this.save_source.borrow_mut().take();
            this.save_now();
        });
        *self.save_source.borrow_mut() = Some(source);
    }

    pub fn flush(&self) {
        if let Some(source) = self.save_source.borrow_mut().take() {
            source.remove();
            self.save_now();
        }
    }

    fn save_now(&self) {
        if let Err(err) = self.config.borrow().save() {
            eprintln!("pharos: could not save config: {err}");
            return;
        }
        // The background process restarts the clocks that changed.
        if ipc::send("reload").is_err() {
            start_background();
        }
    }

    // ------------------------------------------------------------ theme & language

    fn apply_theme(&self) {
        let theme = self.theme.borrow();
        self.provider.load_from_string(&theme.css());
        drop(theme);
        self.relayout();
    }

    /// Debounced: a theme switch touches several files in a row, and
    /// Hyprland applies the new rounding a moment after the files land.
    fn schedule_reload(self: &Rc<Self>) {
        if let Some(source) = self.reload_source.borrow_mut().take() {
            source.remove();
        }
        let this = self.clone();
        let source = glib::timeout_add_local_once(Duration::from_millis(250), move || {
            this.reload_source.borrow_mut().take();
            this.reload_theme_and_language();
            let again = this.clone();
            glib::timeout_add_local_once(Duration::from_millis(1500), move || {
                *again.theme.borrow_mut() = Theme::load();
                again.apply_theme();
            });
        });
        *self.reload_source.borrow_mut() = Some(source);
    }

    fn reload_theme_and_language(self: &Rc<Self>) {
        *self.theme.borrow_mut() = Theme::load();
        self.apply_theme();
        if i18n::reload() {
            self.apply_direction();
            self.relabel();
            self.rebuild();
        }
    }

    fn watch_files(self: &Rc<Self>) {
        let dirs = [
            paths::omarchy_current(),
            paths::omarchy_current().join("theme"),
        ];
        let files = [
            paths::omarchy_user_shell_toml(),
            paths::fontconfig_file(),
            i18n::language_file(),
        ];
        let mut monitors = Vec::new();
        for dir in dirs {
            if let Ok(m) = gio::File::for_path(&dir)
                .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            {
                monitors.push(m);
            }
        }
        for file in files {
            if let Ok(m) = gio::File::for_path(&file)
                .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            {
                monitors.push(m);
            }
        }
        for m in &monitors {
            let this = self.clone();
            m.connect_changed(move |_, _, _, _| this.schedule_reload());
        }
        let watch = |path: std::path::PathBuf| {
            gio::File::for_path(path)
                .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
                .ok()
        };
        if let Some(m) = watch(ipc::state_path()) {
            let this = self.clone();
            m.connect_changed(move |_, _, _, _| this.sync_clocks());
            monitors.push(m);
        }
        if let Some(m) = watch(paths::config_file()) {
            let this = self.clone();
            m.connect_changed(move |_, _, _, _| this.reload_config());
            monitors.push(m);
        }
        *self.monitors.borrow_mut() = monitors;
    }

    fn apply_direction(&self) {
        let dir = if i18n::rtl() {
            gtk::TextDirection::Rtl
        } else {
            gtk::TextDirection::Ltr
        };
        gtk::Widget::set_default_direction(dir);
    }

    // ------------------------------------------------------------ window

    fn window_visible(&self) -> bool {
        self.ui
            .borrow()
            .as_ref()
            .is_some_and(|ui| ui.window.is_visible())
    }

    pub fn present(self: &Rc<Self>) {
        if self.ui.borrow().is_none() {
            self.build_window();
        }
        self.refresh_live();
        let window = self.ui.borrow().as_ref().map(|ui| ui.window.clone());
        if let Some(w) = window {
            // Before present, so the window opens at the edit view's size.
            if std::env::var_os("PHAROS_SNAPSHOT_EDIT").is_some() {
                self.set_editing(true);
            }
            w.present();
            self.apply_cursor(true);
            if let Some(path) = std::env::var_os("PHAROS_SNAPSHOT") {
                glib::timeout_add_local_once(Duration::from_millis(900), move || {
                    snapshot_to_png(&w, std::path::Path::new(&path));
                });
            }
        }
    }

    pub fn toggle_window(self: &Rc<Self>) {
        let visible = self.window_visible();
        if visible {
            let window = self.ui.borrow().as_ref().map(|ui| ui.window.clone());
            if let Some(w) = window {
                w.close();
            }
        } else {
            self.present();
        }
    }

    fn build_window(self: &Rc<Self>) {
        self.apply_direction();
        let window = gtk::ApplicationWindow::builder()
            .application(&self.app)
            .title("Pharos")
            .default_width(480)
            .default_height(640)
            .decorated(false)
            .icon_name(paths::APP_ID)
            .build();
        window.add_css_class("pharos");

        // The column sits in the middle of the window both ways, whatever
        // size the tiling gives it.
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_valign(gtk::Align::Center);

        // Header: title and status, master switch, pencil.
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let titles = gtk::Box::new(gtk::Orientation::Vertical, 2);
        titles.set_hexpand(true);
        titles.set_valign(gtk::Align::Center);
        let title = gtk::Label::new(Some("Pharos"));
        title.add_css_class("app-title");
        title.set_xalign(0.0);
        let status = gtk::Label::new(None);
        status.add_css_class("status");
        status.add_css_class("numeric");
        status.set_xalign(0.0);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        titles.append(&title);
        titles.append(&status);

        let master = gtk::Switch::new();
        master.set_valign(gtk::Align::Center);
        master.set_active(self.config.borrow().enabled);
        let edit_icon = gtk::Label::new(Some(ICON_EDIT));
        let edit_button = gtk::Button::new();
        edit_button.set_child(Some(&edit_icon));
        edit_button.add_css_class("icon");
        edit_button.set_valign(gtk::Align::Center);
        header.append(&titles);
        header.append(&master);
        header.append(&edit_button);

        // Reminder list.
        let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let edit_extras = gtk::Box::new(gtk::Orientation::Vertical, 12);
        edit_extras.set_margin_top(10);
        let add_label = gtk::Label::new(None);
        let add_button = gtk::Button::new();
        add_button.set_child(Some(&add_label));
        add_button.add_css_class("add");
        let autostart = gtk::CheckButton::new();
        autostart.set_active(autostart::enabled());
        autostart.set_halign(gtk::Align::Start);
        edit_extras.append(&add_button);
        edit_extras.append(&autostart);
        edit_extras.set_visible(false);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&list);
        content.append(&edit_extras);
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .child(&content)
            .build();

        let hint = gtk::Label::new(None);
        hint.add_css_class("hint");
        hint.set_wrap(true);
        hint.set_max_width_chars(40);
        hint.set_justify(gtk::Justification::Center);

        root.append(&header);
        root.append(&scroller);
        root.append(&hint);
        window.set_child(Some(&root));

        // Signals.
        let this = self.clone();
        master.connect_active_notify(move |sw| this.set_master(sw.is_active()));
        let this = self.clone();
        edit_button.connect_clicked(move |_| this.set_editing(!this.editing.get()));
        let this = self.clone();
        add_button.connect_clicked(move |_| this.add_reminder());
        autostart.connect_toggled(|c| {
            if let Err(err) = autostart::set(c.is_active()) {
                eprintln!("pharos: could not change autostart: {err}");
            }
        });

        // Follow the size Hyprland gives the window.
        let this = self.clone();
        let last = Cell::new((0, 0));
        window.add_tick_callback(move |w, _| {
            let size = (w.width(), w.height());
            if size != last.get() {
                last.set(size);
                this.relayout();
            }
            glib::ControlFlow::Continue
        });

        // Every open is a new Hyprland window: tag it for Omarchy's window
        // transparency. Hyprland tiles it like any other window.
        let this = self.clone();
        window.connect_map(move |_| {
            crate::hypr::on_open();
            // Fit the list to the tile as soon as Hyprland applies it.
            let this = this.clone();
            glib::timeout_add_local_once(Duration::from_millis(350), move || this.relayout());
        });

        // Closing ends this program; the reminders keep running in the
        // background process.
        let this = self.clone();
        window.connect_close_request(move |_| {
            this.flush();
            this.ui.borrow_mut().take();
            glib::Propagation::Proceed
        });

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = self.clone();
        keys.connect_key_pressed(move |_, key, _, mods| this.on_key(key, mods));
        window.add_controller(keys);

        // Added last: GTK runs a widget's newest controller first, so this
        // one sees every event before the key handler can stop it.
        crate::trim::watch(&window);

        *self.ui.borrow_mut() = Some(Ui {
            window,
            root,
            scroller,
            columns: vec![header.upcast(), content.upcast(), hint.clone().upcast()],
            list,
            rows: Vec::new(),
            master,
            edit_button,
            edit_icon,
            status,
            hint,
            edit_extras,
            add_label,
            autostart,
        });
        self.relabel();
        self.relayout();
        self.rebuild();
    }

    /// Centers a column of at most COLUMN px in whatever size the window
    /// has, and lets the list use the height that's left. Spacing follows
    /// the theme's scale, the way the shell scales its panels.
    fn relayout(&self) {
        let ui = self.ui.borrow();
        let Some(ui) = ui.as_ref() else { return };
        let theme = self.theme.borrow();
        let pad = theme.padding();
        let scale = theme.gap(100.0) as f64 / 100.0;
        let (width, height) = (ui.window.width() as f64, ui.window.height() as f64);
        let column = COLUMN * scale + 2.0 * pad as f64;
        if width > column {
            ui.root.set_halign(gtk::Align::Center);
            ui.root.set_size_request(column.round() as i32, -1);
        } else {
            ui.root.set_halign(gtk::Align::Fill);
            ui.root.set_size_request(-1, -1);
        }
        // Whatever isn't the list (header, hint, edit buttons) keeps its
        // natural height; the list gets the rest.
        let (_, root_h, _, _) = ui.root.measure(gtk::Orientation::Vertical, -1);
        let (_, list_h, _, _) = ui.scroller.measure(gtk::Orientation::Vertical, -1);
        let list = (height - f64::from(root_h - list_h)).max(160.0 * scale);
        ui.scroller.set_max_content_height(list.round() as i32);
        for (i, w) in ui.columns.iter().enumerate() {
            w.set_margin_start(pad);
            w.set_margin_end(pad);
            match i {
                0 => {
                    w.set_margin_top(pad + theme.gap(6.0));
                    w.set_margin_bottom(theme.gap(14.0));
                }
                1 => w.set_margin_bottom(theme.gap(8.0)),
                _ => {
                    w.set_margin_top(theme.gap(6.0));
                    w.set_margin_bottom(pad);
                }
            }
        }
    }

    /// Texts that don't depend on rows.
    fn relabel(&self) {
        let ui = self.ui.borrow();
        let Some(ui) = ui.as_ref() else { return };
        let editing = self.editing.get();
        ui.edit_icon
            .set_text(if editing { ICON_DONE } else { ICON_EDIT });
        let tip = i18n::tr(if editing { "done" } else { "edit" });
        ui.edit_button.set_tooltip_text(Some(&tip));
        ui.edit_button
            .update_property(&[gtk::accessible::Property::Label(&tip)]);
        if editing {
            ui.edit_button.add_css_class("editing");
        } else {
            ui.edit_button.remove_css_class("editing");
        }
        ui.master.set_tooltip_text(Some(&i18n::tr("master")));
        ui.add_label
            .set_text(&format!("{ICON_ADD}  {}", i18n::tr("new_reminder")));
        ui.autostart.set_label(Some(&i18n::tr("start_at_login")));
        ui.hint.set_text(&i18n::tr(if editing {
            "hint_editing"
        } else {
            "hint_normal"
        }));
        ui.edit_extras.set_visible(editing);
    }

    // ------------------------------------------------------------ rows

    fn rebuild(self: &Rc<Self>) {
        let (list, old) = {
            let mut ui = self.ui.borrow_mut();
            let Some(ui) = ui.as_mut() else { return };
            (ui.list.clone(), std::mem::take(&mut ui.rows))
        };
        drop(old);
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }

        let editing = self.editing.get();
        let reminders = self.config.borrow().reminders.clone();
        let mut rows = Vec::with_capacity(reminders.len());
        for r in &reminders {
            let row = self.build_row(r, editing);
            list.append(&row.root);
            rows.push(row);
        }
        if reminders.is_empty() && !editing {
            let empty = gtk::Label::new(Some(&i18n::tr("empty")));
            empty.add_css_class("empty");
            empty.set_wrap(true);
            empty.set_max_width_chars(40);
            empty.set_justify(gtk::Justification::Center);
            list.append(&empty);
        }
        if self.config.borrow().enabled {
            list.remove_css_class("paused");
        } else {
            list.add_css_class("paused");
        }
        if let Some(ui) = self.ui.borrow_mut().as_mut() {
            ui.rows = rows;
        }
        let last = reminders.len().saturating_sub(1);
        self.cursor.set(self.cursor.get().min(last));
        self.apply_cursor(false);
        self.refresh_live();
    }

    fn build_row(self: &Rc<Self>, r: &Reminder, editing: bool) -> Row {
        let theme = self.theme.borrow();
        let root = gtk::Box::new(gtk::Orientation::Vertical, theme.gap(4.0));
        root.add_css_class("reminder");
        if !r.enabled {
            root.add_css_class("off");
        }

        let top = gtk::Box::new(gtk::Orientation::Horizontal, theme.gap(8.0));
        let countdown = gtk::Label::new(None);
        countdown.add_css_class("countdown");
        countdown.add_css_class("numeric");
        let mut entry = None;
        if editing {
            let e = gtk::Entry::new();
            e.set_text(&r.display_name());
            e.set_placeholder_text(Some(&i18n::tr("name_placeholder")));
            e.set_hexpand(true);
            let delete = gtk::Button::new();
            delete.set_child(Some(&gtk::Label::new(Some(ICON_DELETE))));
            delete.add_css_class("icon");
            delete.add_css_class("flat");
            delete.add_css_class("destructive");
            let tip = i18n::tr("delete");
            delete.set_tooltip_text(Some(&tip));
            delete.update_property(&[gtk::accessible::Property::Label(&tip)]);
            top.append(&e);
            top.append(&delete);

            let this = self.clone();
            let id = r.id;
            e.connect_changed(move |e| this.rename(id, &e.text()));
            let this = self.clone();
            delete.connect_clicked(move |_| this.delete(id));
            entry = Some(e);
        } else {
            let name = gtk::Label::new(Some(&r.display_name()));
            name.add_css_class("name");
            name.set_xalign(0.0);
            name.set_hexpand(true);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            top.append(&name);
            top.append(&countdown);
        }

        let bottom = gtk::Box::new(gtk::Orientation::Horizontal, theme.gap(10.0));
        let check = gtk::CheckButton::new();
        check.set_active(r.enabled);
        check.set_valign(gtk::Align::Center);
        check.update_property(&[gtk::accessible::Property::Label(&r.display_name())]);
        let scale = gtk::Scale::with_range(
            gtk::Orientation::Horizontal,
            MIN_MINUTES as f64,
            MAX_MINUTES as f64,
            1.0,
        );
        scale.set_draw_value(false);
        scale.set_digits(0);
        scale.set_round_digits(0);
        scale.set_hexpand(true);
        scale.set_value(r.minutes as f64);
        for mark in [20.0, 40.0, 60.0, 80.0, 100.0] {
            scale.add_mark(mark, gtk::PositionType::Bottom, None);
        }
        let value = gtk::Label::new(Some(&minutes_label(r.minutes)));
        value.add_css_class("value");
        value.add_css_class("numeric");
        value.set_xalign(1.0);
        bottom.append(&check);
        bottom.append(&scale);
        bottom.append(&value);

        root.append(&top);
        root.append(&bottom);

        // Signals are connected after the initial values so building a row
        // never writes the config.
        let id = r.id;
        let this = self.clone();
        check.connect_toggled(move |c| this.set_enabled(id, c.is_active()));
        let this = self.clone();
        scale.connect_value_changed(move |s| this.set_minutes(id, s.value().round() as u32));

        let this = self.clone();
        let motion = gtk::EventControllerMotion::new();
        motion.connect_enter(move |_, _, _| this.cursor_to(id, false));
        root.add_controller(motion);
        let this = self.clone();
        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter(move |_| this.cursor_to(id, false));
        root.add_controller(focus);

        Row {
            id,
            root,
            check,
            scale,
            value,
            countdown,
            entry,
        }
    }

    fn row_widget<T>(&self, index: usize, f: impl Fn(&Row) -> T) -> Option<T> {
        self.ui.borrow().as_ref()?.rows.get(index).map(f)
    }

    fn cursor_to(&self, id: u32, focus: bool) {
        let index = self
            .ui
            .borrow()
            .as_ref()
            .and_then(|ui| ui.rows.iter().position(|r| r.id == id));
        if let Some(i) = index {
            if i != self.cursor.get() || focus {
                self.cursor.set(i);
                self.apply_cursor(focus);
            }
        }
    }

    fn apply_cursor(&self, focus: bool) {
        let target = {
            let ui = self.ui.borrow();
            let Some(ui) = ui.as_ref() else { return };
            for (i, row) in ui.rows.iter().enumerate() {
                if i == self.cursor.get() {
                    row.root.add_css_class("cursor");
                } else {
                    row.root.remove_css_class("cursor");
                }
            }
            ui.rows.get(self.cursor.get()).map(|r| r.scale.clone())
        };
        if focus {
            if let Some(scale) = target {
                scale.grab_focus();
            }
        }
    }

    fn move_cursor(&self, delta: isize) {
        let len = self.ui.borrow().as_ref().map_or(0, |ui| ui.rows.len());
        if len == 0 {
            return;
        }
        let next = (self.cursor.get() as isize + delta).clamp(0, len as isize - 1);
        self.cursor.set(next as usize);
        self.apply_cursor(true);
    }

    /// Countdowns and the header status; runs every second while visible.
    fn refresh_live(&self) {
        let ui = self.ui.borrow();
        let Some(ui) = ui.as_ref() else { return };
        let cfg = self.config.borrow();
        let sched = self.scheduler.borrow();
        let now = SystemTime::now();
        for row in &ui.rows {
            let Some(r) = cfg.reminders.iter().find(|r| r.id == row.id) else {
                continue;
            };
            let text = if !cfg.enabled {
                String::new()
            } else if !r.enabled {
                i18n::tr("off")
            } else {
                sched
                    .remaining(r.id, now)
                    .map(scheduler::clock)
                    .unwrap_or_default()
            };
            row.countdown.set_text(&text);
        }
        let status = if !cfg.enabled {
            i18n::tr("paused")
        } else if let Some((id, left)) = sched.next(&cfg, now) {
            let name = cfg
                .reminders
                .iter()
                .find(|r| r.id == id)
                .map(Reminder::display_name)
                .unwrap_or_default();
            i18n::trf(
                "next",
                &[("name", &name), ("time", &scheduler::clock(left))],
            )
        } else {
            i18n::tr("nothing_scheduled")
        };
        ui.status.set_text(&status);
    }

    // ------------------------------------------------------------ edits

    fn set_master(self: &Rc<Self>, on: bool) {
        {
            let mut cfg = self.config.borrow_mut();
            if cfg.enabled == on {
                return;
            }
            cfg.enabled = on;
            self.scheduler
                .borrow_mut()
                .restart_all(&cfg, SystemTime::now());
        }
        self.show_master(on);
        self.schedule_save();
        self.refresh_live();
    }

    /// Puts the switch and the dimmed list in step with `on`.
    fn show_master(&self, on: bool) {
        let switch = self.ui.borrow().as_ref().map(|ui| {
            if on {
                ui.list.remove_css_class("paused");
            } else {
                ui.list.add_css_class("paused");
            }
            ui.master.clone()
        });
        // The switch's handler returns early: the config already matches.
        if let Some(sw) = switch {
            if sw.is_active() != on {
                sw.set_active(on);
            }
        }
    }

    fn set_enabled(self: &Rc<Self>, id: u32, on: bool) {
        match self.config.borrow_mut().get_mut(id) {
            Some(r) if r.enabled != on => r.enabled = on,
            _ => return,
        }
        self.restart_clock(id);
        if let Some(ui) = self.ui.borrow().as_ref() {
            if let Some(row) = ui.rows.iter().find(|r| r.id == id) {
                if on {
                    row.root.remove_css_class("off");
                } else {
                    row.root.add_css_class("off");
                }
            }
        }
        self.schedule_save();
        self.refresh_live();
    }

    fn set_minutes(self: &Rc<Self>, id: u32, minutes: u32) {
        let minutes = minutes.clamp(MIN_MINUTES, MAX_MINUTES);
        match self.config.borrow_mut().get_mut(id) {
            Some(r) if r.minutes != minutes => r.minutes = minutes,
            _ => return,
        }
        // A new interval starts counting from now.
        self.restart_clock(id);
        if let Some(ui) = self.ui.borrow().as_ref() {
            if let Some(row) = ui.rows.iter().find(|r| r.id == id) {
                row.value.set_text(&minutes_label(minutes));
            }
        }
        self.schedule_save();
        self.refresh_live();
    }

    fn rename(self: &Rc<Self>, id: u32, text: &str) {
        if let Some(r) = self.config.borrow_mut().get_mut(id) {
            r.rename(text);
        }
        self.schedule_save();
        self.refresh_live();
    }

    fn delete(self: &Rc<Self>, id: u32) {
        self.config.borrow_mut().reminders.retain(|r| r.id != id);
        self.scheduler.borrow_mut().remove(id);
        self.schedule_save();
        // Rebuild after the click handler returns: the button is removed.
        let this = self.clone();
        glib::idle_add_local_once(move || {
            this.rebuild();
            this.apply_cursor(true);
        });
    }

    fn add_reminder(self: &Rc<Self>) {
        let id = {
            let mut cfg = self.config.borrow_mut();
            let id = cfg.next_id();
            cfg.reminders
                .push(Reminder::preset(id, i18n::NEW_PRESET, 30, true));
            id
        };
        self.restart_clock(id);
        self.schedule_save();
        if !self.editing.get() {
            self.editing.set(true);
            self.relabel();
        }
        self.rebuild();
        self.cursor_to(id, false);
        self.focus_name();
    }

    fn focus_name(&self) {
        let entry = self
            .row_widget(self.cursor.get(), |r| r.entry.clone())
            .flatten();
        if let Some(e) = entry {
            e.grab_focus();
            e.select_region(0, -1);
        }
    }

    fn set_editing(self: &Rc<Self>, on: bool) {
        if self.editing.replace(on) == on {
            return;
        }
        self.flush();
        self.relabel();
        self.rebuild();
        self.apply_cursor(true);
    }

    fn toggle_cursor_row(&self) {
        if let Some(check) = self.row_widget(self.cursor.get(), |r| r.check.clone()) {
            check.set_active(!check.is_active());
        }
    }

    fn adjust_cursor_row(&self, delta: f64) {
        if let Some(scale) = self.row_widget(self.cursor.get(), |r| r.scale.clone()) {
            scale.set_value(scale.value() + delta);
        }
    }

    // ------------------------------------------------------------ keyboard

    fn on_key(self: &Rc<Self>, key: gdk::Key, mods: gdk::ModifierType) -> glib::Propagation {
        use gdk::Key;
        use glib::Propagation::{Proceed, Stop};

        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
        let Some(window) = self.ui.borrow().as_ref().map(|ui| ui.window.clone()) else {
            return Proceed;
        };
        let focus = gtk::prelude::GtkWindowExt::focus(&window);
        let in_text = focus.as_ref().is_some_and(|w| {
            w.is::<gtk::Text>() || w.ancestor(gtk::Entry::static_type()).is_some()
        });
        let on_control = focus.as_ref().is_some_and(|w| {
            w.is::<gtk::Button>() || w.is::<gtk::CheckButton>() || w.is::<gtk::Switch>()
        });

        if ctrl && matches!(key, Key::w | Key::W) {
            window.close();
            return Stop;
        }
        if key == Key::Escape {
            if in_text {
                self.apply_cursor(true);
            } else if self.editing.get() {
                self.set_editing(false);
            } else {
                window.close();
            }
            return Stop;
        }
        if in_text {
            if matches!(key, Key::Return | Key::KP_Enter | Key::Down | Key::Up) {
                self.apply_cursor(true);
                if key == Key::Down {
                    self.move_cursor(1);
                } else if key == Key::Up {
                    self.move_cursor(-1);
                }
                return Stop;
            }
            return Proceed;
        }
        if ctrl || mods.contains(gdk::ModifierType::ALT_MASK) {
            return Proceed;
        }

        let step = if shift { 5.0 } else { 1.0 };
        match key {
            Key::j | Key::Down => self.move_cursor(1),
            Key::k | Key::Up => self.move_cursor(-1),
            Key::g | Key::Home => self.move_cursor(isize::MIN / 2),
            Key::G | Key::End => self.move_cursor(isize::MAX / 2),
            Key::h | Key::Left => self.adjust_cursor_row(-step),
            Key::l | Key::Right => self.adjust_cursor_row(step),
            Key::H => self.adjust_cursor_row(-5.0),
            Key::L => self.adjust_cursor_row(5.0),
            Key::space | Key::Return | Key::KP_Enter if !on_control => self.toggle_cursor_row(),
            Key::e => self.set_editing(!self.editing.get()),
            Key::n => self.add_reminder(),
            Key::t => {
                let master = self.ui.borrow().as_ref().map(|ui| ui.master.clone());
                if let Some(m) = master {
                    m.set_active(!m.is_active());
                }
            }
            Key::r | Key::F2 if self.editing.get() => self.focus_name(),
            Key::x | Key::Delete if self.editing.get() => {
                if let Some(id) = self.row_widget(self.cursor.get(), |r| r.id) {
                    self.delete(id);
                }
            }
            _ => return Proceed,
        }
        Stop
    }
}

fn minutes_label(n: u32) -> String {
    i18n::trf("minutes", &[("n", &n.to_string())])
}

/// Debug aid: `PHAROS_SNAPSHOT=out.png pharos` renders the window to a PNG,
/// for screenshots without screen-capture permissions.
fn snapshot_to_png(window: &gtk::ApplicationWindow, path: &std::path::Path) {
    let paintable = gtk::WidgetPaintable::new(Some(window));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
    let (Some(node), Some(renderer)) = (snapshot.to_node(), window.renderer()) else {
        return;
    };
    let texture = renderer.render_texture(&node, None);
    if let Err(err) = texture.save_to_png(path) {
        eprintln!("pharos: snapshot failed: {err}");
    }
}

/// Starts the background process if it isn't running (the window was
/// opened directly), so the reminders run after the window closes.
fn start_background() {
    let exe = std::env::current_exe()
        .ok()
        .map(|e| e.with_file_name("pharos"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| "pharos".into());
    if let Ok(mut child) = std::process::Command::new(exe)
        .arg("--background")
        .stdin(std::process::Stdio::null())
        .spawn()
    {
        std::thread::spawn(move || child.wait());
    }
}
