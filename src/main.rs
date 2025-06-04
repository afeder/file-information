use gtk::prelude::*;
use gtk::{CssProvider, Grid, Label, TextView, Widget};
use gtk::pango;
use gtk::WrapMode as GtkWrapMode;
use gio::{Cancellable, ApplicationFlags};
use gdk4::Rectangle;
use gdk4::Display;
use adw::prelude::*;
use adw::{Application, ApplicationWindow, HeaderBar, ToolbarView};
use tracker::prelude::SparqlCursorExtManual;
use tracker::SparqlConnection;
use std::collections::HashMap;
use glib::{Variant, VariantTy, Propagation};
use std::env;

const APP_ID: &str = "com.example.FileInformation";
const TOOLTIP_MAX_CHARS: usize = 80;
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const XSD_DATETYPE: &str = "http://www.w3.org/2001/XMLSchema#dateType";
const FILEDATAOBJECT: &str = "http://tracker.api.gnome.org/ontology/v3/nfo#FileDataObject";

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
        .default_width(600)
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

    let scroll = gtk::ScrolledWindow::builder()
        .min_content_width(600)
        .min_content_height(400)
        .child(&grid)
        .build();

    let toolbar = ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scroll));
    window.set_content(Some(&toolbar));
    window.present();

    let app_clone = app.clone();

    let is_file_data_object = populate_grid(&app_clone, &window, &grid, &uri);

    header_label.set_text(
        if is_file_data_object {
            "File Information"
        } else {
            "Resource Information"
        }
    );
}

fn populate_grid(
    app: &Application,
    window: &ApplicationWindow,
    grid: &Grid,
    uri: &str,
) -> bool {
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }

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

    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);

    let disp_clone = uri.to_string();
    let native_clone = uri.to_string();
    let widget_clone: Widget = uri_label.clone().upcast();

    gesture.connect_pressed(move |_gesture, _n_press, x, y| {
        let menu_model = gio::Menu::new();

        let copy_disp_item = gio::MenuItem::new(
            Some("Copy Displayed Value"),
            Some("win.copy-displayed-value"),
        );
        let disp_variant = Variant::from(disp_clone.as_str());
        copy_disp_item.set_attribute_value("target", Some(&disp_variant));
        menu_model.append_item(&copy_disp_item);

        let copy_nat_item = gio::MenuItem::new(
            Some("Copy Native Value"),
            Some("win.copy-native-value"),
        );
        let nat_variant = Variant::from(native_clone.as_str());
        copy_nat_item.set_attribute_value("target", Some(&nat_variant));
        menu_model.append_item(&copy_nat_item);

        let popover = gtk::PopoverMenu::from_model(Some(&menu_model));

        let rect = Rectangle::new(x as i32, y as i32, 1, 1);
        popover.set_pointing_to(Some(&rect));

        popover.set_parent(&widget_clone);
        popover.popup();
    });

    uri_label.add_controller(gesture);

    let tooltip_text = ellipsize(uri, TOOLTIP_MAX_CHARS);
    uri_label.set_tooltip_text(Some(&tooltip_text));

    grid.attach(&id_label, 0, 0, 1, 1);
    grid.attach(&uri_label, 1, 0, 1, 1);

    let conn = match SparqlConnection::bus_new(
        "org.freedesktop.Tracker3.Miner.Files",
        None,
        None,
    ) {
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
            return false;
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
            return false;
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
        map.get_mut(&pred).unwrap().push((obj.clone(), dtype.clone()));

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

                    let gesture = gtk::GestureClick::new();
                    gesture.set_button(3);

                    let disp_clone = label_text.clone();
                    let native_clone = pred.clone();
                    let widget_clone: Widget = lbl_key.clone().upcast();

                    gesture.connect_pressed(move |_gesture, _n_press, x, y| {
                        let menu_model = gio::Menu::new();

                        let copy_disp_item = gio::MenuItem::new(
                            Some("Copy Displayed Predicate"),
                            Some("win.copy-displayed-value"),
                        );
                        let disp_variant = Variant::from(&disp_clone as &str);
                        copy_disp_item
                            .set_attribute_value("target", Some(&disp_variant));
                        menu_model.append_item(&copy_disp_item);

                        let copy_nat_item = gio::MenuItem::new(
                            Some("Copy Native Predicate"),
                            Some("win.copy-native-value"),
                        );
                        let nat_variant = Variant::from(&native_clone as &str);
                        copy_nat_item
                            .set_attribute_value("target", Some(&nat_variant));
                        menu_model.append_item(&copy_nat_item);

                        let popover = gtk::PopoverMenu::from_model(Some(&menu_model));

                        let rect = Rectangle::new(x as i32, y as i32, 1, 1);
                        popover.set_pointing_to(Some(&rect));

                        popover.set_parent(&widget_clone);
                        popover.popup();
                    });
                    lbl_key.add_controller(gesture);

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

                        let gesture = gtk::GestureClick::new();
                        gesture.set_button(3);

                        let disp_clone = displayed_str.clone();
                        let native_clone = native_str.clone();
                        let widget_clone: Widget = lbl_val.clone().upcast();

                        gesture.connect_pressed(
                            move |_gesture, _n_press, x, y| {
                                let menu_model = gio::Menu::new();

                                let copy_disp_item = gio::MenuItem::new(
                                    Some("Copy Displayed Value"),
                                    Some("win.copy-displayed-value"),
                                );
                                let disp_variant =
                                    Variant::from(&disp_clone as &str);
                                copy_disp_item
                                    .set_attribute_value("target", Some(&disp_variant));
                                menu_model.append_item(&copy_disp_item);

                                let copy_nat_item = gio::MenuItem::new(
                                    Some("Copy Native Value"),
                                    Some("win.copy-native-value"),
                                );
                                let nat_variant =
                                    Variant::from(&native_clone as &str);
                                copy_nat_item
                                    .set_attribute_value("target", Some(&nat_variant));
                                menu_model.append_item(&copy_nat_item);

                                let popover =
                                    gtk::PopoverMenu::from_model(Some(&menu_model));

                                let rect =
                                    Rectangle::new(x as i32, y as i32, 1, 1);
                                popover.set_pointing_to(Some(&rect));

                                popover.set_parent(&widget_clone);
                                popover.popup();
                            },
                        );

                        lbl_val.add_controller(gesture);
                        lbl_val.upcast()
                    }
                };

                let tooltip_text = ellipsize(&native_str, TOOLTIP_MAX_CHARS);
                widget.set_tooltip_text(Some(&tooltip_text));

                grid.attach(&widget, 1, row, 1, 1);
                row += 1;
            }
        }
    }
    is_file_data_object
}

fn friendly_label(uri: &str) -> String {
    let last = uri.rsplit(&['#', '/'][..]).next().unwrap_or(uri);
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
    fn friendly_label_basic() {
        let uri = "https://example.com/FooBarBaz";
        assert_eq!(friendly_label(uri), "Foo Bar Baz");
    }
}
