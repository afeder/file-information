use adw::prelude::*;
use adw::{Application, ApplicationWindow, HeaderBar, ToolbarView};
use csv::WriterBuilder;
use gdk4::Display;
use gdk4::Rectangle;
use gio::{ApplicationFlags, Cancellable};
use glib::{Propagation, Variant, VariantTy};
use gtk::WrapMode as GtkWrapMode;
use gtk::pango;
use gtk::{Box as GtkBox, Button, CssProvider, Grid, Label, Orientation, TextView, Widget};
use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::rc::Rc;
use tracker::SparqlConnection;
use tracker::prelude::SparqlCursorExtManual;
use url::Url;

const APP_ID: &str = "com.example.FileInformation";
const TOOLTIP_MAX_CHARS: usize = 80;
const COMMENT_TOOLTIP_MAX_CHARS: usize = TOOLTIP_MAX_CHARS * 3;
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const XSD_DATETYPE: &str = "http://www.w3.org/2001/XMLSchema#dateType";
const RDFS_COMMENT: &str = "http://www.w3.org/2000/01/rdf-schema#comment";
const FILEDATAOBJECT: &str = "http://tracker.api.gnome.org/ontology/v3/nfo#FileDataObject";

#[derive(Clone, Default)]
struct TableRow {
    display_predicate: String,
    native_predicate: String,
    display_value: String,
    native_value: String,
}

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let mut raw_uri = false;
    if let Some(first) = args.first().map(|s| s.as_str()) {
        if first == "-u" || first == "--uri" {
            raw_uri = true;
            args.remove(0);
        }
    }

    let app = Application::builder()
        .application_id(APP_ID)
        .flags(ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    app.connect_command_line(move |app, cmd_line| {
        let argv = cmd_line.arguments();
        let inputs: Vec<String> = argv
            .iter()
            .skip(1)
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        let mut raw = raw_uri;
        let mut items = inputs.clone();
        if let Some(first) = items.first().map(|s| s.as_str()) {
            if first == "-u" || first == "--uri" {
                raw = true;
                items.remove(0);
            }
        }
        if let Some(id) = items.first() {
            let uri = if raw {
                id.clone()
            } else {
                gio::File::for_path(id).uri().to_string()
            };
            app.activate();
            build_ui(app, uri.clone());
            0
        } else {
            eprintln!("Usage: file-information [--uri|-u] <file-or-URI>");
            1
        }
    });

    app.connect_activate(|_| {});

    app.connect_open(move |app, files, _| {
        if let Some(file) = files.first() {
            build_ui(app, file.uri().to_string());
        }
    });

    app.run();
}

fn build_ui(app: &Application, uri: String) {
    let window = ApplicationWindow::builder()
        .application(app)
        .default_width(590)
        .default_height(400)
        .title("File Information")
        .build();

    let copy_disp = gio::SimpleAction::new("copy-displayed-value", Some(&VariantTy::STRING));
    copy_disp.connect_activate(move |_action, param| {
        if let Some(v) = param {
            if let Some(text) = v.str() {
                if let Some(display) = Display::default() {
                    let clipboard = display.clipboard();
                    clipboard.set_text(text);
                }
            }
        }
    });
    window.add_action(&copy_disp);

    let copy_nat = gio::SimpleAction::new("copy-native-value", Some(&VariantTy::STRING));
    copy_nat.connect_activate(move |_action, param| {
        if let Some(v) = param {
            if let Some(text) = v.str() {
                if let Some(display) = Display::default() {
                    let clipboard = display.clipboard();
                    clipboard.set_text(text);
                }
            }
        }
    });
    window.add_action(&copy_nat);

    let win_for_uri = window.clone();
    let open_uri_action = gio::SimpleAction::new("open-uri", Some(&VariantTy::STRING));
    open_uri_action.connect_activate(move |_action, param| {
        if let Some(v) = param {
            if let Some(uri) = v.str() {
                let report = |msg: String| {
                    let dialog = gtk::MessageDialog::builder()
                        .transient_for(&win_for_uri)
                        .modal(true)
                        .message_type(gtk::MessageType::Info)
                        .buttons(gtk::ButtonsType::Ok)
                        .text("Could not open URI")
                        .secondary_text(&msg)
                        .build();
                    dialog.connect_response(|dlg, _| dlg.close());
                    dialog.show();
                };

                if let Err(msg) = uri_has_handler(uri) {
                    report(msg);
                    return;
                }

                if let Err(err) =
                    gio::AppInfo::launch_default_for_uri(uri, None::<&gio::AppLaunchContext>)
                {
                    report(err.to_string());
                }
            }
        }
    });
    window.add_action(&open_uri_action);

    let provider = CssProvider::new();
    let css = r#"
        grid#data-grid {
            background-color: transparent;
            margin: 0;
            padding: 0;
        }
        label.first-col {
            font-weight: bold;
        }
        textview.bordered {
            border: 1px solid @separator_color;
            padding: 4px;
            margin-right: 6px;
        }
    "#;
    provider.load_from_data(css);
    if let Some(display) = Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let header = HeaderBar::new();
    header.set_show_end_title_buttons(true);

    let header_label = Label::new(Some("Loading…"));
    header.set_title_widget(Some(&header_label));

    let grid = Grid::builder()
        .column_homogeneous(false)
        .hexpand(true)
        .vexpand(true)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Fill)
        .build();
    grid.set_widget_name("data-grid");

    let viewport = gtk::Viewport::builder()
        .scroll_to_focus(false)
        .child(&grid)
        .build();

    let scroll = gtk::ScrolledWindow::builder()
        .min_content_width(590)
        .min_content_height(400)
        .child(&viewport)
        .build();

    let toolbar = ToolbarView::new();
    toolbar.add_top_bar(&header);

    let table_data: Rc<RefCell<Vec<TableRow>>> = Rc::new(RefCell::new(Vec::new()));

    let close_button = Button::with_label("Close");
    let win_clone = window.clone();
    close_button.connect_clicked(move |_| {
        win_clone.close();
    });

    let copy_button = Button::with_label("Copy");
    let data_clone = table_data.clone();
    copy_button.connect_clicked(move |_| {
        let rows = data_clone.borrow();
        let mut wtr = WriterBuilder::new().has_headers(true).from_writer(vec![]);
        let _ = wtr.write_record([
            "Display Predicate",
            "Native Predicate",
            "Display Value",
            "Native Value",
        ]);
        for r in rows.iter() {
            let _ = wtr.write_record([
                &r.display_predicate,
                &r.native_predicate,
                &r.display_value,
                &r.native_value,
            ]);
        }
        if let Ok(data) = String::from_utf8(wtr.into_inner().unwrap_or_default()) {
            if let Some(display) = Display::default() {
                display.clipboard().set_text(&data);
            }
        }
    });

    let open_button = Button::with_label("Open");
    let win_for_action = window.clone();
    let uri_clone = uri.clone();
    open_button.connect_clicked(move |_| {
        gio::prelude::ActionGroupExt::activate_action(
            &win_for_action,
            "open-uri",
            Some(&Variant::from(uri_clone.as_str())),
        );
    });

    let backlinks_button = Button::with_label("Backlinks");
    let app_clone = app.clone();
    let win_parent = window.clone();
    let uri_bl = uri.clone();
    backlinks_button.connect_clicked(move |_| {
        show_backlinks_window(&app_clone, &win_parent, uri_bl.clone());
    });

    let bottom_box = GtkBox::new(Orientation::Horizontal, 0);
    bottom_box.set_spacing(5);
    bottom_box.set_halign(gtk::Align::End);
    bottom_box.set_margin_start(6);
    bottom_box.set_margin_end(6);
    bottom_box.set_margin_top(6);
    bottom_box.set_margin_bottom(6);
    bottom_box.append(&backlinks_button);
    bottom_box.append(&copy_button);
    if uri_has_handler(&uri).is_ok() {
        bottom_box.append(&open_button);
    }
    bottom_box.append(&close_button);
    toolbar.add_bottom_bar(&bottom_box);

    toolbar.set_content(Some(&scroll));
    window.set_content(Some(&toolbar));
    window.present();

    let app_clone = app.clone();

    let (is_file_data_object, rows) = populate_grid(&app_clone, &window, &grid, &uri);
    table_data.borrow_mut().clear();
    table_data.borrow_mut().extend(rows);

    header_label.set_text(if is_file_data_object {
        "File Information"
    } else {
        "Node Information"
    });
}

fn populate_grid(
    app: &Application,
    window: &ApplicationWindow,
    grid: &Grid,
    uri: &str,
) -> (bool, Vec<TableRow>) {
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }

    let mut rows_vec = Vec::new();

    let id_label = Label::new(Some("Identifier"));
    id_label.set_halign(gtk::Align::Start);
    id_label.set_valign(gtk::Align::Start);
    id_label.style_context().add_class("first-col");
    id_label.set_margin_start(6);
    id_label.set_margin_top(4);
    id_label.set_margin_bottom(4);

    let uri_label = Label::new(Some(uri));
    uri_label.set_halign(gtk::Align::Start);
    uri_label.set_margin_start(6);
    uri_label.set_margin_top(4);
    uri_label.set_margin_bottom(4);
    uri_label.set_wrap(true);
    uri_label.set_wrap_mode(pango::WrapMode::WordChar);
    uri_label.set_max_width_chars(80);

    add_copy_menu(
        &uri_label,
        uri,
        uri,
        "Copy Displayed Value",
        "Copy Native Value",
    );

    let tooltip_text = ellipsize(uri, TOOLTIP_MAX_CHARS);
    uri_label.set_tooltip_text(Some(&tooltip_text));

    grid.attach(&id_label, 0, 0, 1, 1);
    grid.attach(&uri_label, 1, 0, 1, 1);
    rows_vec.push(TableRow {
        display_predicate: "Identifier".to_string(),
        native_predicate: "Identifier".to_string(),
        display_value: uri.to_string(),
        native_value: uri.to_string(),
    });

    let conn = match SparqlConnection::bus_new("org.freedesktop.Tracker3.Miner.Files", None, None) {
        Ok(c) => c,
        Err(err) => {
            let dialog = gtk::MessageDialog::builder()
                .transient_for(window)
                .modal(true)
                .message_type(gtk::MessageType::Error)
                .text("Failed to connect to Tracker")
                .secondary_text(&format!("{err}"))
                .buttons(gtk::ButtonsType::Ok)
                .build();
            dialog.connect_response(|dlg, _| dlg.close());
            dialog.show();
            return (false, Vec::new());
        }
    };

    let sparql = format!(
        r#"
        SELECT DISTINCT ?pred ?obj (DATATYPE(?obj) AS ?dtype) WHERE {{
            <{uri}> ?pred ?obj .
        }}
    "#,
        uri = uri
    );
    let cursor = match conn.query(&sparql, None::<&Cancellable>) {
        Ok(c) => c,
        Err(err) => {
            let dialog = gtk::MessageDialog::builder()
                .transient_for(window)
                .modal(true)
                .message_type(gtk::MessageType::Error)
                .text("SPARQL query error")
                .secondary_text(&format!("{err}"))
                .buttons(gtk::ButtonsType::Ok)
                .build();
            dialog.connect_response(|dlg, _| dlg.close());
            dialog.show();
            return (false, Vec::new());
        }
    };

    let mut order = Vec::new();
    let mut map: HashMap<String, Vec<(String, String)>> = HashMap::new();

    let mut is_file_data_object = false;

    while cursor.next(None::<&Cancellable>).unwrap_or(false) {
        let pred = cursor.string(0).unwrap_or_default().to_string();
        let obj = cursor.string(1).unwrap_or_default().to_string();
        let dtype = cursor.string(2).unwrap_or_default().to_string();
        if !map.contains_key(&pred) {
            order.push(pred.clone());
            map.insert(pred.clone(), Vec::new());
        }
        map.get_mut(&pred)
            .unwrap()
            .push((obj.clone(), dtype.clone()));

        if pred == RDF_TYPE && obj == FILEDATAOBJECT {
            is_file_data_object = true;
        }
    }

    let mut row = 1;
    for pred in order {
        if let Some(entries) = map.get(&pred) {
            let label_text = friendly_label(&pred);

            for (i, (obj, dtype)) in entries.iter().enumerate() {
                if i == 0 {
                    let lbl_key = Label::new(Some(&label_text));
                    lbl_key.set_halign(gtk::Align::Start);
                    lbl_key.set_valign(gtk::Align::Start);
                    lbl_key.style_context().add_class("first-col");
                    lbl_key.set_tooltip_text(Some(&pred));
                    lbl_key.set_margin_start(6);
                    lbl_key.set_margin_top(4);
                    lbl_key.set_margin_bottom(4);

                    add_copy_menu(
                        &lbl_key,
                        &label_text,
                        &pred,
                        "Copy Displayed Predicate",
                        "Copy Native Predicate",
                    );

                    let lbl_key_clone = lbl_key.clone();
                    let pred_clone = pred.clone();
                    let gesture = gtk::GestureClick::new();
                    gesture.set_button(1);
                    gesture.connect_pressed(move |_, _, _, _| {
                        if let Some(comment) = fetch_comment(&pred_clone) {
                            let tip = ellipsize(&comment, COMMENT_TOOLTIP_MAX_CHARS);
                            lbl_key_clone.set_tooltip_text(Some(&tip));
                            let lbl_ref = lbl_key_clone.clone();
                            glib::idle_add_local_once(move || {
                                lbl_ref.trigger_tooltip_query();
                            });
                        }
                    });
                    lbl_key.add_controller(gesture);

                    let lbl_key_leave = lbl_key.clone();
                    let pred_leave = pred.clone();
                    let motion = gtk::EventControllerMotion::new();
                    motion.connect_leave(move |_| {
                        lbl_key_leave.set_tooltip_text(Some(&pred_leave));
                    });
                    lbl_key.add_controller(motion);

                    grid.attach(&lbl_key, 0, row, 1, 1);
                }

                let displayed_str = if dtype.is_empty() {
                    obj.clone()
                } else {
                    friendly_value(obj, dtype)
                };
                let native_str = obj.clone();

                let widget: gtk::Widget = if dtype.is_empty() {
                    let lbl_link = Label::new(None);
                    lbl_link.set_markup(&format!("<a href=\"{0}\">{0}</a>", obj));
                    lbl_link.set_halign(gtk::Align::Start);
                    lbl_link.set_margin_start(6);
                    lbl_link.set_margin_top(4);
                    lbl_link.set_margin_bottom(4);

                    let app_clone = app.clone();
                    lbl_link.connect_activate_link(move |_lbl, uri| {
                        build_ui(&app_clone, uri.to_string());
                        Propagation::Stop
                    });

                    lbl_link.set_wrap(true);
                    lbl_link.set_wrap_mode(pango::WrapMode::WordChar);
                    lbl_link.set_max_width_chars(80);

                    add_copy_menu(
                        &lbl_link,
                        &displayed_str,
                        &native_str,
                        "Copy Displayed Value",
                        "Copy Native Value",
                    );

                    lbl_link.upcast()
                } else {
                    if obj.contains('\n') {
                        let txt = TextView::new();
                        txt.set_editable(false);
                        txt.set_cursor_visible(false);
                        txt.style_context().add_class("bordered");
                        txt.set_wrap_mode(GtkWrapMode::Word);
                        txt.set_margin_start(6);
                        txt.set_margin_end(9);
                        txt.set_margin_top(4);
                        txt.set_margin_bottom(4);

                        let buffer = txt.buffer();
                        buffer.set_text(&displayed_str);
                        let start = buffer.start_iter();
                        buffer.place_cursor(&start);
                        txt.upcast()
                    } else {
                        let lbl_val = Label::new(Some(&displayed_str));
                        lbl_val.set_halign(gtk::Align::Start);
                        lbl_val.set_margin_start(6);
                        lbl_val.set_margin_top(4);
                        lbl_val.set_margin_bottom(4);
                        lbl_val.set_wrap(true);
                        lbl_val.set_wrap_mode(pango::WrapMode::WordChar);
                        lbl_val.set_max_width_chars(80);

                        add_copy_menu(
                            &lbl_val,
                            &displayed_str,
                            &native_str,
                            "Copy Displayed Value",
                            "Copy Native Value",
                        );
                        lbl_val.upcast()
                    }
                };

                let tooltip_text = ellipsize(&native_str, TOOLTIP_MAX_CHARS);
                widget.set_tooltip_text(Some(&tooltip_text));

                grid.attach(&widget, 1, row, 1, 1);
                rows_vec.push(TableRow {
                    display_predicate: label_text.clone(),
                    native_predicate: pred.clone(),
                    display_value: displayed_str.clone(),
                    native_value: native_str.clone(),
                });
                row += 1;
            }
        }
    }
    (is_file_data_object, rows_vec)
}

fn friendly_label(uri: &str) -> String {
    let trimmed = uri.trim_end_matches(&['#', '/'][..]);
    let last = trimmed.rsplit(&['#', '/'][..]).next().unwrap_or(trimmed);
    let mut words = Vec::new();
    let mut cur = String::new();
    for c in last.chars() {
        if c.is_uppercase() && !cur.is_empty() {
            words.push(cur.clone());
            cur.clear();
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
        .into_iter()
        .map(|w| {
            let mut cs = w.chars();
            if let Some(f) = cs.next() {
                f.to_uppercase().collect::<String>() + cs.as_str()
            } else {
                String::new()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn friendly_value(obj: &str, dtype: &str) -> String {
    if dtype == XSD_DATETYPE {
        if let Ok(dt) = glib::DateTime::from_iso8601(obj, None)
            .and_then(|dt| dt.to_local())
            .and_then(|ldt| ldt.format("%F %T"))
        {
            return dt.to_string();
        }
    }
    obj.to_string()
}

fn ellipsize(s: &str, max_chars: usize) -> String {
    let mut count = 0;
    let mut result = String::new();
    for ch in s.chars() {
        if count >= max_chars {
            result.push('…');
            break;
        }
        result.push(ch);
        count += 1;
    }
    if count < s.chars().count() {
        result
    } else {
        s.to_string()
    }
}

fn looks_like_uri(s: &str) -> bool {
    Url::parse(s).is_ok()
}

fn uri_has_handler(uri: &str) -> Result<(), String> {
    if let Ok(url) = Url::parse(uri) {
        if url.scheme() == "file" {
            if let Ok(path) = url.to_file_path() {
                if let Some(p) = path.to_str() {
                    let (mime, _) = gio::content_type_guess(Some(p), b"");
                    if gio::AppInfo::default_for_type(&mime, false).is_none() {
                        return Err(format!("No application available for type \"{}\".", mime));
                    }
                }
            }
        } else if gio::AppInfo::default_for_uri_scheme(url.scheme()).is_none() {
            return Err(format!(
                "No application available for scheme \"{}\".",
                url.scheme()
            ));
        }
    }
    Ok(())
}

fn add_copy_menu<W>(widget: &W, displayed: &str, native: &str, disp_label: &str, nat_label: &str)
where
    W: IsA<gtk::Widget> + Clone + 'static,
{
    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    gesture.set_exclusive(true);
    gesture.set_propagation_phase(gtk::PropagationPhase::Capture);

    let disp_clone = displayed.to_string();
    let native_clone = native.to_string();
    let disp_label_str = disp_label.to_string();
    let nat_label_str = nat_label.to_string();
    let widget_clone: Widget = widget.clone().upcast();

    gesture.connect_pressed(move |_gesture, _n_press, x, y| {
        let menu_model = gio::Menu::new();

        let copy_disp_item =
            gio::MenuItem::new(Some(&disp_label_str), Some("win.copy-displayed-value"));
        let disp_variant = Variant::from(disp_clone.as_str());
        copy_disp_item.set_attribute_value("target", Some(&disp_variant));
        menu_model.append_item(&copy_disp_item);

        let copy_nat_item = gio::MenuItem::new(Some(&nat_label_str), Some("win.copy-native-value"));
        let nat_variant = Variant::from(native_clone.as_str());
        copy_nat_item.set_attribute_value("target", Some(&nat_variant));
        menu_model.append_item(&copy_nat_item);

        if looks_like_uri(&native_clone) && uri_has_handler(&native_clone).is_ok() {
            let open_item = gio::MenuItem::new(Some("Open Externally"), Some("win.open-uri"));
            let uri_variant = Variant::from(native_clone.as_str());
            open_item.set_attribute_value("target", Some(&uri_variant));
            menu_model.append_item(&open_item);
        }

        let popover = gtk::PopoverMenu::from_model(Some(&menu_model));

        let (parent, rect) = if let Some(root) = widget_clone.root() {
            if let Some((rx, ry)) = widget_clone.translate_coordinates(&root, x, y) {
                (
                    root.upcast::<Widget>(),
                    Rectangle::new(rx as i32, ry as i32, 1, 1),
                )
            } else {
                (
                    root.upcast::<Widget>(),
                    Rectangle::new(x as i32, y as i32, 1, 1),
                )
            }
        } else {
            (
                widget_clone.clone(),
                Rectangle::new(x as i32, y as i32, 1, 1),
            )
        };

        popover.set_parent(&parent);
        popover.set_pointing_to(Some(&rect));
        popover.popup();
    });

    widget.add_controller(gesture);
}

fn show_backlinks_window(app: &Application, parent: &ApplicationWindow, uri: String) {
    let window = ApplicationWindow::builder()
        .application(app)
        .transient_for(parent)
        .default_width(590)
        .default_height(400)
        .title("Backlinks")
        .build();

    let header = HeaderBar::new();
    header.set_show_end_title_buttons(true);
    let header_label = Label::new(Some("Backlinks"));
    header.set_title_widget(Some(&header_label));

    let grid = Grid::builder()
        .column_homogeneous(false)
        .hexpand(true)
        .vexpand(true)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Fill)
        .build();
    grid.set_widget_name("data-grid");

    let viewport = gtk::Viewport::builder()
        .scroll_to_focus(false)
        .child(&grid)
        .build();

    let scroll = gtk::ScrolledWindow::builder()
        .min_content_width(590)
        .min_content_height(400)
        .child(&viewport)
        .build();

    let toolbar = ToolbarView::new();
    toolbar.add_top_bar(&header);

    let close_button = Button::with_label("Close");
    let win_clone = window.clone();
    close_button.connect_clicked(move |_| {
        win_clone.close();
    });

    let bottom_box = GtkBox::new(Orientation::Horizontal, 0);
    bottom_box.set_spacing(5);
    bottom_box.set_halign(gtk::Align::End);
    bottom_box.set_margin_start(6);
    bottom_box.set_margin_end(6);
    bottom_box.set_margin_top(6);
    bottom_box.set_margin_bottom(6);
    bottom_box.append(&close_button);
    toolbar.add_bottom_bar(&bottom_box);

    toolbar.set_content(Some(&scroll));
    window.set_content(Some(&toolbar));
    window.present();

    populate_backlinks_grid(app, &window, &grid, &uri);
}

fn populate_backlinks_grid(
    app: &Application,
    window: &ApplicationWindow,
    grid: &Grid,
    uri: &str,
) {
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }

    let conn = match SparqlConnection::bus_new("org.freedesktop.Tracker3.Miner.Files", None, None) {
        Ok(c) => c,
        Err(err) => {
            let dialog = gtk::MessageDialog::builder()
                .transient_for(window)
                .modal(true)
                .message_type(gtk::MessageType::Error)
                .text("Failed to connect to Tracker")
                .secondary_text(&format!("{err}"))
                .buttons(gtk::ButtonsType::Ok)
                .build();
            dialog.connect_response(|dlg, _| dlg.close());
            dialog.show();
            return;
        }
    };

    let sparql = format!(
        "SELECT DISTINCT ?s ?p WHERE {{ ?s ?p <{uri}> }}",
        uri = uri
    );
    let cursor = match conn.query(&sparql, None::<&Cancellable>) {
        Ok(c) => c,
        Err(err) => {
            let dialog = gtk::MessageDialog::builder()
                .transient_for(window)
                .modal(true)
                .message_type(gtk::MessageType::Error)
                .text("SPARQL query error")
                .secondary_text(&format!("{err}"))
                .buttons(gtk::ButtonsType::Ok)
                .build();
            dialog.connect_response(|dlg, _| dlg.close());
            dialog.show();
            return;
        }
    };

    let mut row = 0;
    while cursor.next(None::<&Cancellable>).unwrap_or(false) {
        let subj = cursor.string(0).unwrap_or_default().to_string();
        let pred = cursor.string(1).unwrap_or_default().to_string();

        let widget: Widget = if looks_like_uri(&subj) {
            let lbl_link = Label::new(None);
            lbl_link.set_markup(&format!("<a href=\"{0}\">{0}</a>", subj));
            lbl_link.set_halign(gtk::Align::Start);
            lbl_link.set_margin_start(6);
            lbl_link.set_margin_top(4);
            lbl_link.set_margin_bottom(4);
            lbl_link.set_wrap(true);
            lbl_link.set_wrap_mode(pango::WrapMode::WordChar);
            lbl_link.set_max_width_chars(80);

            let app_clone = app.clone();
            lbl_link.connect_activate_link(move |_lbl, uri| {
                build_ui(&app_clone, uri.to_string());
                Propagation::Stop
            });

            lbl_link.upcast()
        } else {
            let lbl_val = Label::new(Some(&subj));
            lbl_val.set_halign(gtk::Align::Start);
            lbl_val.set_margin_start(6);
            lbl_val.set_margin_top(4);
            lbl_val.set_margin_bottom(4);
            lbl_val.set_wrap(true);
            lbl_val.set_wrap_mode(pango::WrapMode::WordChar);
            lbl_val.set_max_width_chars(80);
            lbl_val.upcast()
        };

        widget.set_tooltip_text(Some(&subj));
        grid.attach(&widget, 0, row, 1, 1);

        let lbl_pred = Label::new(Some(&friendly_label(&pred)));
        lbl_pred.set_halign(gtk::Align::Start);
        lbl_pred.set_valign(gtk::Align::Start);
        lbl_pred.style_context().add_class("first-col");
        lbl_pred.set_tooltip_text(Some(&pred));
        lbl_pred.set_margin_start(6);
        lbl_pred.set_margin_top(4);
        lbl_pred.set_margin_bottom(4);

        grid.attach(&lbl_pred, 1, row, 1, 1);
        row += 1;
    }
}

fn fetch_comment(predicate: &str) -> Option<String> {
    let conn =
        SparqlConnection::bus_new("org.freedesktop.Tracker3.Miner.Files", None, None).ok()?;
    let sparql = format!(
        "SELECT ?c WHERE {{ <{pred}> <{comment}> ?c }} LIMIT 1",
        pred = predicate,
        comment = RDFS_COMMENT
    );
    let cursor = conn.query(&sparql, None::<&Cancellable>).ok()?;
    if cursor.next(None::<&Cancellable>).unwrap_or(false) {
        Some(cursor.string(0).unwrap_or_default().to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ellipsize_shorter_than_limit() {
        let input = "a".repeat(TOOLTIP_MAX_CHARS - 1);
        assert_eq!(ellipsize(&input, TOOLTIP_MAX_CHARS), input);
    }

    #[test]
    fn ellipsize_equal_to_limit() {
        let input = "a".repeat(TOOLTIP_MAX_CHARS);
        assert_eq!(ellipsize(&input, TOOLTIP_MAX_CHARS), input);
    }

    #[test]
    fn ellipsize_longer_than_limit() {
        let input = "a".repeat(TOOLTIP_MAX_CHARS + 5);
        let expected = format!("{}…", "a".repeat(TOOLTIP_MAX_CHARS));
        assert_eq!(ellipsize(&input, TOOLTIP_MAX_CHARS), expected);
    }

    #[test]
    fn ellipsize_multibyte_characters() {
        let input = "é".repeat(TOOLTIP_MAX_CHARS + 2);
        let expected = format!("{}…", "é".repeat(TOOLTIP_MAX_CHARS));
        assert_eq!(ellipsize(&input, TOOLTIP_MAX_CHARS), expected);
    }

    #[test]
    fn friendly_label_basic() {
        let uri = "https://example.com/FooBarBaz";
        assert_eq!(friendly_label(uri), "Foo Bar Baz");
    }

    #[test]
    fn friendly_label_trailing_slash() {
        let uri = "https://example.com/FooBarBaz/";
        assert_eq!(friendly_label(uri), "Foo Bar Baz");
    }

    #[test]
    fn friendly_label_trailing_hash() {
        let uri = "https://example.com/FooBarBaz#";
        assert_eq!(friendly_label(uri), "Foo Bar Baz");
    }

    #[test]
    fn friendly_value_formats_date() {
        let raw = "2024-06-04T12:34:56Z";
        let expected = glib::DateTime::from_iso8601(raw, None)
            .and_then(|dt| dt.to_local())
            .and_then(|ldt| ldt.format("%F %T"))
            .unwrap();
        assert_eq!(friendly_value(raw, XSD_DATETYPE), expected);
    }

    #[test]
    fn friendly_value_invalid_date() {
        let raw = "invalid";
        assert_eq!(friendly_value(raw, XSD_DATETYPE), raw);
    }

    #[test]
    fn friendly_value_unrelated_type() {
        let raw = "hello";
        assert_eq!(friendly_value(raw, "other"), raw);
    }

    #[test]
    fn looks_like_uri_valid() {
        assert!(looks_like_uri("https://example.com"));
    }

    #[test]
    fn looks_like_uri_invalid() {
        assert!(!looks_like_uri("not a uri"));
    }

    #[test]
    fn looks_like_uri_date() {
        assert!(!looks_like_uri("2024-06-04T12:34:56Z"));
    }
}
