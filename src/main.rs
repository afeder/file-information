// Import the Adwaita GTK4 extensions prelude which provides traits needed for
// builder patterns and widget manipulation.
use adw::prelude::*;
// Bring in the high level Adwaita application and widget types used throughout
// the UI such as the application object itself, main windows and header bars.
use adw::{Application, ApplicationWindow, HeaderBar, ToolbarView};
// CSV is used when copying all rows to the clipboard so that the content can be
// easily pasted into spreadsheets. `WriterBuilder` allows us to construct a
// writer with custom settings.
use csv::WriterBuilder;
// GDK is the layer below GTK that deals with display server interaction. We use
// it for clipboard access and positioning popover menus.
use gdk4::Display;
use gdk4::Rectangle;
// GIO provides asynchronous IO facilities and the `ApplicationFlags` used when
// creating the `Application` as well as `Cancellable` for cancellation support
// with Tracker queries.
use gio::{ApplicationFlags, Cancellable};
// GLib offers miscellaneous utilities. `Variant` is used for passing parameters
// to actions and `Propagation` is used when linking labels.
use glib::{Propagation, Variant, VariantTy};
// GTK types used for building the UI. `WrapMode` is aliased to avoid a name
// clash with the pango module.
use gtk::WrapMode as GtkWrapMode;
// The pango module provides text layout utilities such as word wrapping.
use gtk::pango;
// Various GTK widgets used in the interface. `Box` is renamed since Rust does
// not allow importing two different items with the same name.
use gtk::{Box as GtkBox, Button, CssProvider, Grid, Label, Orientation, TextView, Widget};
// Standard library utilities. `RefCell` and `Rc` are used for interior mutable
// shared state, `HashMap` for collecting query results and `env` for argument
// handling.
use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::rc::Rc;
// Tracker is queried via SPARQL to obtain metadata about the given URI.
use tracker::SparqlConnection;
use tracker::prelude::SparqlCursorExtManual;
// The `url` crate helps determine if strings look like URIs and to check for
// handlers based on URI scheme.
use url::Url;

// Identifier used when registering the application with the desktop session.
// This must match the ID declared in the desktop file for proper integration.
const APP_ID: &str = "com.example.FileInformation";

/// Command line usage string printed when the help flag is given or the
/// invocation is missing required arguments.
const USAGE: &str = "Usage: file-information [--uri|-u] [--debug|-d] <file-or-URI>";

// Tooltips can become very verbose for long values. The user can still copy the
// full value, so we limit the tooltip length to keep the UI readable.
const TOOLTIP_MAX_CHARS: usize = 80;

// Comments fetched from the ontology tend to be long sentences so we use a
// slightly larger ellipsization limit for those tooltips.
const COMMENT_TOOLTIP_MAX_CHARS: usize = TOOLTIP_MAX_CHARS * 3;

// Various ontology constants used when interpreting Tracker results.
// `RDF_TYPE` is the standard predicate for the type/class of a resource.
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

// `XSD_DATETYPE` identifies date values in Tracker and is referenced in the
// repository guidelines. When we see this datatype we format the date nicely.
const XSD_DATETYPE: &str = "http://www.w3.org/2001/XMLSchema#dateType";

// Used when requesting an rdfs:comment for a predicate to provide explanatory
// tooltips about properties.
const RDFS_COMMENT: &str = "http://www.w3.org/2000/01/rdf-schema#comment";

// Tracker stores file nodes with this type so we can adapt the UI title when we
// detect it.
const FILEDATAOBJECT: &str = "http://tracker.api.gnome.org/ontology/v3/nfo#FileDataObject";

/// A row of data shown in the table. We keep both the human readable and raw
/// versions of the predicate and value so that the user can choose which to
/// copy when invoking the context menu.
#[derive(Clone, Default)]
struct TableRow {
    /// User facing predicate name shown in the UI.
    display_predicate: String,
    /// Native URI of the predicate as returned by Tracker.
    native_predicate: String,
    /// Formatted representation of the value (dates, links etc.).
    display_value: String,
    /// Raw value exactly as returned by Tracker.
    native_value: String,
}

/// Entry point. Parses command line arguments and sets up the main `Application`.
///
/// Supported flags:
/// * `-u` / `--uri`  - interpret the provided argument as a URI rather than a
///   filesystem path.
/// * `-d` / `--debug` - print additional diagnostic information to stderr.
fn main() {
    // Collect all command line arguments except the binary name.
    let mut args: Vec<String> = env::args().skip(1).collect();

    if args.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!("{}", USAGE);
        return;
    }

    // Flags which influence how we interpret the first argument and whether to
    // emit debug output.
    let mut raw_uri = false;
    let mut debug_flag = false;

    // Manually parse the first arguments so that they can be specified before
    // or after the filename. We modify `args` in place removing processed flags
    // until no more recognised options remain.
    loop {
        match args.first().map(|s| s.as_str()) {
            Some("-u") | Some("--uri") => {
                raw_uri = true;
                args.remove(0);
            }
            Some("-d") | Some("--debug") => {
                debug_flag = true;
                args.remove(0);
            }
            _ => break,
        }
    }

    // Construct the Adwaita application. `HANDLES_COMMAND_LINE` allows us to
    // parse additional arguments when the app is activated via the command line
    // rather than via `gtk_launch` or the desktop shell.
    let app = Application::builder()
        .application_id(APP_ID)
        .flags(ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    // Handle the `command-line` signal which fires when the application is
    // started from a terminal. This allows one instance to process multiple
    // invocations.
    app.connect_command_line(move |app, cmd_line| {
        // Convert the raw arguments to owned strings for easier manipulation.
        let argv = cmd_line.arguments();
        let inputs: Vec<String> = argv
            .iter()
            .skip(1)
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        if inputs.iter().any(|a| a == "-h" || a == "--help") {
            eprintln!("{}", USAGE);
            return 0;
        }
        let mut raw = raw_uri;
        let mut debug = debug_flag;
        let mut items = inputs.clone();
        loop {
            match items.first().map(|s| s.as_str()) {
                Some("-u") | Some("--uri") => {
                    raw = true;
                    items.remove(0);
                }
                Some("-d") | Some("--debug") => {
                    debug = true;
                    items.remove(0);
                }
                _ => break,
            }
        }
        // After processing flags, the first remaining item is treated as the
        // target file or URI.
        if let Some(id) = items.first() {
            let uri = if raw {
                id.clone()
            } else {
                gio::File::for_path(id).uri().to_string()
            };
            app.activate();
            build_ui(app, uri.clone(), debug);
            0
        } else {
            eprintln!("{}", USAGE);
            1
        }
    });

    // We handle activation explicitly inside `connect_command_line` and
    // `connect_open`, so the default `activate` handler does nothing.
    app.connect_activate(|_| {});

    // When the application is launched by opening a file from the file manager
    // this signal provides the `gio::File` objects.
    app.connect_open(move |app, files, _| {
        if let Some(file) = files.first() {
            build_ui(app, file.uri().to_string(), debug_flag);
        }
    });

    // Enter the GTK main loop. From this point on all logic is driven by
    // callbacks connected above.
    app.run();
}

/// Build and present the main window showing metadata for the provided `uri`.
///
/// The UI consists of a simple two column grid inside an Adwaita `ToolbarView`.
/// Additional actions such as copying data or opening URIs are added as global
/// window actions so they can be reused from multiple widgets.
fn build_ui(app: &Application, uri: String, debug: bool) {
    let window = ApplicationWindow::builder()
        .application(app)
        .default_width(590)
        .default_height(400)
        .title("File Information")
        .build();

    // Install common actions (copy/open) on the newly created window so that
    // any widget can trigger them via the action system.
    add_common_actions(&window);

    // Apply a small CSS snippet to control padding and emphasise the first
    // column. Doing this in code avoids having to ship a separate stylesheet.
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
    // Register the CSS provider so the styling above is applied application
    // wide. The display is optional but should always exist in normal GUI
    // environments.
    if let Some(display) = Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    // Header bar containing the dynamic title and close/open buttons.
    let header = HeaderBar::new();
    header.set_show_end_title_buttons(true);

    let header_label = Label::new(Some("Loading…"));
    header.set_title_widget(Some(&header_label));

    // Grid where metadata rows will be inserted. We disable homogeneous columns
    // so that the first column can be as narrow as its contents require.
    let grid = Grid::builder()
        .column_homogeneous(false)
        .hexpand(true)
        .vexpand(true)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Fill)
        .build();
    grid.set_widget_name("data-grid");

    // Wrapping the grid in a viewport ensures scrolling works correctly even
    // when focus is moved inside it.
    let viewport = gtk::Viewport::builder()
        .scroll_to_focus(false)
        .child(&grid)
        .build();

    // Outer scrolled window provides scrollbars when there are many rows.
    let scroll = gtk::ScrolledWindow::builder()
        .min_content_width(590)
        .min_content_height(400)
        .child(&viewport)
        .build();

    // Adwaita's ToolbarView gives us a simple layout with a header and bottom
    // bar while keeping the content scrollable.
    let toolbar = ToolbarView::new();
    toolbar.add_top_bar(&header);

    // Shared container for the rows currently displayed. This is later used
    // when the user chooses to copy all values to the clipboard.
    let table_data: Rc<RefCell<Vec<TableRow>>> = Rc::new(RefCell::new(Vec::new()));

    // Button row at the bottom of the window. `Close` simply dismisses the
    // window.
    let close_button = Button::with_label("Close");
    let win_clone = window.clone();
    close_button.connect_clicked(move |_| {
        win_clone.close();
    });

    // `Copy` exports the table to CSV and places it on the clipboard so the
    // user can paste it elsewhere.
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

    // `Open` delegates to the `open-uri` action which performs sanity checks
    // before launching an external application.
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

    // `Backlinks` opens a secondary window listing all triples that reference
    // the current URI. Helpful for understanding relationships between nodes.
    let backlinks_button = Button::with_label("Backlinks");
    let app_clone = app.clone();
    let win_parent = window.clone();
    let uri_bl = uri.clone();
    let debug_clone = debug;
    backlinks_button.connect_clicked(move |_| {
        show_backlinks_window(&app_clone, &win_parent, uri_bl.clone(), debug_clone);
    });

    // Container holding the action buttons at the bottom right of the window.
    let bottom_box = GtkBox::new(Orientation::Horizontal, 0);
    bottom_box.set_spacing(5);
    bottom_box.set_halign(gtk::Align::End);
    bottom_box.set_margin_start(6);
    bottom_box.set_margin_end(6);
    bottom_box.set_margin_top(6);
    bottom_box.set_margin_bottom(6);
    // Order of buttons: Backlinks, Copy, optionally Open, and finally Close.
    bottom_box.append(&backlinks_button);
    bottom_box.append(&copy_button);
    // Only show the Open button if there is a registered handler for the URI.
    if uri_has_handler(&uri).is_ok() {
        bottom_box.append(&open_button);
    }
    bottom_box.append(&close_button);
    toolbar.add_bottom_bar(&bottom_box);

    toolbar.set_content(Some(&scroll));
    window.set_content(Some(&toolbar));
    window.present();

    let app_clone = app.clone();
    let window_clone = window.clone();
    let grid_clone = grid.clone();
    let header_clone = header_label.clone();
    let data_clone = table_data.clone();
    let uri_clone = uri.clone();

    // Load the data asynchronously so the UI remains responsive while Tracker
    // queries are running. `spawn_local` runs the future on the main thread's
    // event loop.
    glib::MainContext::default().spawn_local(async move {
        let (is_file_data_object, rows) =
            populate_grid(&app_clone, &window_clone, &grid_clone, &uri_clone, debug).await;
        let row_count = rows.len().saturating_sub(1);
        data_clone.borrow_mut().clear();
        data_clone.borrow_mut().extend(rows);

        // Update the window title once data arrives so users know whether the
        // URI corresponds to an actual file or just a generic resource.
        header_clone.set_text(if is_file_data_object {
            "File Information"
        } else {
            "Node Information"
        });

        if debug {
            // To debug performance we hook into the frame clock so we can log
            // exactly when the rows become visible.
            if let Some(clock) = grid_clone.frame_clock() {
                use std::cell::RefCell;
                use gdk4::FrameClockPhase;

                // We connect to the AFTER_PAINT phase only once. The handler
                // removes itself after logging.
                let handler: Rc<RefCell<Option<glib::SignalHandlerId>>> =
                    Rc::new(RefCell::new(None));
                let handler_clone = handler.clone();
                let id = clock.connect_after_paint(move |clk| {
                    if let Some(h) = handler_clone.borrow_mut().take() {
                        clk.disconnect(h);
                    }
                    // Print a debug line once the grid has actually been
                    // painted. Useful when benchmarking query latency vs.
                    // rendering time.
                    eprintln!(
                        "DEBUG: results displayed rows={} file_data={}",
                        row_count,
                        is_file_data_object
                    );
                });
                *handler.borrow_mut() = Some(id);
                clock.request_phase(FrameClockPhase::AFTER_PAINT);
            }
        }
    });
}

/// Query Tracker for all triples associated with `uri` and populate the grid
/// with the results. Returns a flag indicating whether the node represents a
/// regular file and the rows displayed.
async fn populate_grid(
    app: &Application,
    window: &ApplicationWindow,
    grid: &Grid,
    uri: &str,
    debug: bool,
) -> (bool, Vec<TableRow>) {
    // Remove any previous rows so we start with a clean grid each time this is
    // called (e.g. when switching URIs).
    // Clear any existing children from previous queries.
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }
    if debug {
        eprintln!("Fetching backlinks for {uri}");
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

    if debug {
        eprintln!("Connecting to Tracker miner for metadata…");
    }
    // Establish a connection to Tracker's SPARQL endpoint over the session bus.
    let conn = match SparqlConnection::bus_new("org.freedesktop.Tracker3.Miner.Files", None, None) {
        Ok(c) => c,
        Err(err) => {
            if debug {
                eprintln!("Failed to connect to Tracker: {err}");
            }
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

    // Query all predicates and objects for the given URI. Tracker will return
    // literals with their datatype which allows us to prettify certain values
    // such as dates.
    let sparql = format!(
        r#"
        SELECT DISTINCT ?pred ?obj (DATATYPE(?obj) AS ?dtype) WHERE {{
            <{uri}> ?pred ?obj .
        }}
    "#,
        uri = uri
    );
    if debug {
        eprintln!("Running SPARQL query: {sparql}");
    }
    let cursor = match conn.query_future(&sparql).await {
        Ok(c) => c,
        Err(err) => {
            // Report errors to both the user and stderr when debugging.
            if debug {
                eprintln!("SPARQL query error: {err}");
            }
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

    // Keep insertion order of predicates to maintain a stable UI ordering.
    let mut order = Vec::new();
    let mut map: HashMap<String, Vec<(String, String)>> = HashMap::new();

    // Indicates whether the resource is an nfo:FileDataObject which we use
    // later to adjust the title.
    let mut is_file_data_object = false;

    // Iterate through all rows returned by the SPARQL query.
    // Each result row describes one subject that links to our URI together with
    // the predicate used. Iterate through them and add rows to the grid.
    while cursor.next_future().await.unwrap_or(false) {
        let pred = cursor.string(0).unwrap_or_default().to_string();
        let obj = cursor.string(1).unwrap_or_default().to_string();
        let dtype = cursor.string(2).unwrap_or_default().to_string();
        // Group values by predicate while remembering the original order in
        // which predicates appear in the query results.
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
                    // Only add the predicate label once even if there are
                    // multiple values.
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
                    // Left-clicking the predicate attempts to fetch its
                    // rdfs:comment and shows it as a tooltip.
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
                    // When the pointer leaves reset the tooltip to show the
                    // predicate URI again.
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

                // Render URIs as clickable links, otherwise show plain text or
                // a read-only text view if the value contains newlines.
                let widget: gtk::Widget = if dtype.is_empty() {
                    let lbl_link = Label::new(None);
                    let escaped = glib::markup_escape_text(obj);
                    lbl_link.set_markup(&format!("<a href=\"{0}\">{0}</a>", escaped));
                    lbl_link.set_halign(gtk::Align::Start);
                    lbl_link.set_margin_start(6);
                    lbl_link.set_margin_top(4);
                    lbl_link.set_margin_bottom(4);

                    // Clicking the link opens a new window showing the linked
                    // resource. Returning `Propagation::Stop` prevents the
                    // default handler from opening the URI externally.
                    let app_clone = app.clone();
                    let debug_clone = debug;
                    lbl_link.connect_activate_link(move |_lbl, uri| {
                        build_ui(&app_clone, uri.to_string(), debug_clone);
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
                        // Multi-line literals are displayed in a read-only
                        // text view to preserve formatting.
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

                // Show the full native value as a tooltip, truncated for
                // readability.
                let tooltip_text = ellipsize(&native_str, TOOLTIP_MAX_CHARS);
                widget.set_tooltip_text(Some(&tooltip_text));

                grid.attach(&widget, 1, row, 1, 1);
                // Store the row so it can later be copied to the clipboard.
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
    if debug {
        // Log how many rows we collected and whether the node represents a
        // file. Useful when comparing with the number eventually displayed.
        eprintln!(
            "DEBUG: query returned rows={} file_data={}",
            rows_vec.len() - 1,
            is_file_data_object
        );
    }
    (is_file_data_object, rows_vec)
}

/// Produce a human friendly label from a URI by splitting on `/` and `#` and
/// inserting spaces before capital letters.
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

/// Format certain literal values to be easier to read. Currently only date
/// strings are handled.
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

/// Truncate a string to `max_chars` characters adding an ellipsis when data is
/// omitted. Works with multi-byte UTF-8 characters.
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

/// Heuristically determine whether a string is a URI. Used when deciding if a
/// value should be clickable.
fn looks_like_uri(s: &str) -> bool {
    Url::parse(s).is_ok()
}

/// Check whether a URI can be opened with an external application. Returns an
/// error message suitable for display if no handler exists.
fn uri_has_handler(uri: &str) -> Result<(), String> {
    if let Ok(url) = Url::parse(uri) {
        if url.scheme() == "file" {
            // For local files we look up a default handler based on MIME type.
            if let Ok(path) = url.to_file_path() {
                if let Some(p) = path.to_str() {
                    let (mime, _) = gio::content_type_guess(Some(p), b"");
                    if gio::AppInfo::default_for_type(&mime, false).is_none() {
                        return Err(format!("No application available for type \"{}\".", mime));
                    }
                }
            }
        } else if gio::AppInfo::default_for_uri_scheme(url.scheme()).is_none() {
            // For non-file URIs consult registered handlers for the scheme.
            return Err(format!(
                "No application available for scheme \"{}\".",
                url.scheme()
            ));
        }
    }
    Ok(())
}

/// Install actions on the given window so that context menus and buttons can
/// trigger common functionality such as copying values or opening URIs.
fn add_common_actions(window: &ApplicationWindow) {
    // Copy the displayed (formatted) text to the clipboard.
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

    // Copy the underlying raw value.
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

    // Action used to open URIs externally after checking for a handler.
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

                // Launch the appropriate application for the URI. Errors are
                // reported to the user via a dialog.
                if let Err(err) =
                    gio::AppInfo::launch_default_for_uri(uri, None::<&gio::AppLaunchContext>)
                {
                    report(err.to_string());
                }
            }
        }
    });
    window.add_action(&open_uri_action);
}

/// Attach a right-click menu to `widget` offering to copy either the displayed
/// or native value and optionally open it externally.
fn add_copy_menu<W>(widget: &W, displayed: &str, native: &str, disp_label: &str, nat_label: &str)
where
    W: IsA<gtk::Widget> + Clone + 'static,
{
    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    // Only react to right-clicks on this widget and prevent the event from
    // propagating further so default handlers don't run.
    gesture.set_exclusive(true);
    gesture.set_propagation_phase(gtk::PropagationPhase::Capture);

    let disp_clone = displayed.to_string();
    let native_clone = native.to_string();
    let disp_label_str = disp_label.to_string();
    let nat_label_str = nat_label.to_string();
    let widget_clone: Widget = widget.clone().upcast();

    // Build and show the context menu when the gesture is triggered.
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

        // Determine the position to anchor the popover. If the widget is
        // realized we translate the click coordinates into the root window
        // coordinate space.
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

/// Open a secondary window listing all triples that reference `uri`.
fn show_backlinks_window(app: &Application, parent: &ApplicationWindow, uri: String, debug: bool) {
    // Create a modal window positioned above the main one.
    let window = ApplicationWindow::builder()
        .application(app)
        .transient_for(parent)
        .default_width(590)
        .default_height(400)
        .title("Backlinks")
        .build();

    add_common_actions(&window);

    let header = HeaderBar::new();
    header.set_show_end_title_buttons(true);
    let header_label = Label::new(Some("Backlinks"));
    header.set_title_widget(Some(&header_label));

    // Grid used to display each backlink and the predicate via which it refers
    // to the original node.
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

    let app_clone = app.clone();
    let window_clone = window.clone();
    let grid_clone = grid.clone();
    let uri_clone = uri.clone();
    let debug_clone = debug;

    // Load backlinks asynchronously so the UI appears instantly while the
    // query runs in the background.
    glib::MainContext::default().spawn_local(async move {
        populate_backlinks_grid(&app_clone, &window_clone, &grid_clone, &uri_clone, debug_clone).await;
    });
}

/// Populate the backlinks window with triples where `uri` appears as the object.
async fn populate_backlinks_grid(app: &Application, window: &ApplicationWindow, grid: &Grid, uri: &str, debug: bool) {
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }

    let conn = match SparqlConnection::bus_new("org.freedesktop.Tracker3.Miner.Files", None, None) {
        Ok(c) => c,
        Err(err) => {
            if debug {
                eprintln!("Failed to connect to Tracker: {err}");
            }
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

    let sparql = format!("SELECT DISTINCT ?s ?p WHERE {{ ?s ?p <{uri}> }}", uri = uri);
    if debug {
        eprintln!("Running SPARQL query: {sparql}");
    }
    let cursor = match conn.query_future(&sparql).await {
        Ok(c) => c,
        Err(err) => {
            if debug {
                eprintln!("SPARQL query error: {err}");
            }
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
    while cursor.next_future().await.unwrap_or(false) {
        let subj = cursor.string(0).unwrap_or_default().to_string();
        let pred = cursor.string(1).unwrap_or_default().to_string();

        // If the subject itself looks like a URI render it as a clickable link,
        // otherwise display it as plain text.
        let widget: Widget = if looks_like_uri(&subj) {
            let lbl_link = Label::new(None);
            let escaped = glib::markup_escape_text(&subj);
            lbl_link.set_markup(&format!("<a href=\"{0}\">{0}</a>", escaped));
            lbl_link.set_halign(gtk::Align::Start);
            lbl_link.set_margin_start(6);
            lbl_link.set_margin_top(4);
            lbl_link.set_margin_bottom(4);
            lbl_link.set_wrap(true);
            lbl_link.set_wrap_mode(pango::WrapMode::WordChar);
            lbl_link.set_max_width_chars(80);

            let app_clone = app.clone();
            let debug_clone = debug;
            lbl_link.connect_activate_link(move |_lbl, uri| {
                build_ui(&app_clone, uri.to_string(), debug_clone);
                Propagation::Stop
            });

            add_copy_menu(
                &lbl_link,
                &subj,
                &subj,
                "Copy Displayed Value",
                "Copy Native Value",
            );

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

            add_copy_menu(
                &lbl_val,
                &subj,
                &subj,
                "Copy Displayed Value",
                "Copy Native Value",
            );

            lbl_val.upcast()
        };

        widget.set_tooltip_text(Some(&subj));
        grid.attach(&widget, 0, row, 1, 1);

        let pred_label = friendly_label(&pred);
        let lbl_pred = Label::new(Some(&pred_label));
        lbl_pred.set_halign(gtk::Align::Start);
        lbl_pred.set_valign(gtk::Align::Start);
        lbl_pred.style_context().add_class("first-col");
        lbl_pred.set_tooltip_text(Some(&pred));
        lbl_pred.set_margin_start(6);
        lbl_pred.set_margin_top(4);
        lbl_pred.set_margin_bottom(4);

        add_copy_menu(
            &lbl_pred,
            &pred_label,
            &pred,
            "Copy Displayed Predicate",
            "Copy Native Predicate",
        );

        grid.attach(&lbl_pred, 1, row, 1, 1);
        row += 1;
    }
    if debug {
        // Log how many backlinks were found for troubleshooting purposes.
        eprintln!("Backlinks query returned {row} rows");
    }
}

/// Fetch an optional `rdfs:comment` for a predicate to display as tooltip help.
fn fetch_comment(predicate: &str) -> Option<String> {
    // Connect synchronously to Tracker. Errors simply result in None being returned.
    let conn =
        SparqlConnection::bus_new("org.freedesktop.Tracker3.Miner.Files", None, None).ok()?;
    let sparql = format!(
        "SELECT ?c WHERE {{ <{pred}> <{comment}> ?c }} LIMIT 1",
        pred = predicate,
        comment = RDFS_COMMENT
    );
    // Only request a single comment.
    let cursor = conn.query(&sparql, None::<&Cancellable>).ok()?;
    if cursor.next(None::<&Cancellable>).unwrap_or(false) {
        // Return the first comment string if found.
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

    #[test]
    fn ellipsize_zero_limit() {
        assert_eq!(ellipsize("hello", 0), "…");
    }

    #[test]
    fn ellipsize_empty_string() {
        assert_eq!(ellipsize("", 5), "");
    }

    #[test]
    fn friendly_label_domain_only() {
        let uri = "https://example.com";
        assert_eq!(friendly_label(uri), "Example.com");
    }

    #[test]
    fn looks_like_uri_file_scheme() {
        assert!(looks_like_uri("file:///tmp/test"));
    }
}
