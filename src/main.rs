// The application is built with GTK4/Adwaita.  Most widgets live in the `adw`
// and `gtk` crates.  We also communicate with GNOME Tracker over D-Bus using
// the `tracker` crate and issue SPARQL queries.
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
use tracker::prelude::SparqlCursorExtManual;
use tracker::SparqlConnection;
use url::Url;

// Application ID used when creating the `adw::Application`.  This needs to be
// unique so that DBus can correctly identify our instance.
const APP_ID: &str = "com.example.FileInformation";

// Text shown when the user passes `--help`.  We keep it minimal because the
// application is primarily graphical.
const USAGE: &str = "Usage: file-information [--uri|-u] [--debug|-d] <file-or-URI>";

// Maximum number of characters to show in tooltips before truncating with an
// ellipsis.  Tooltips should stay small so they do not obscure the UI.
const TOOLTIP_MAX_CHARS: usize = 80;

// Some tooltips display verbose comments from the ontology.  We allow these to
// be a little longer than regular tooltips.
const COMMENT_TOOLTIP_MAX_CHARS: usize = TOOLTIP_MAX_CHARS * 3;

// Common RDF predicate used to state the type/class of a resource.
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

// Tracker encodes dates using the following XML schema datatype URI.  It is
// important not to rename this constant as tests depend on it.
const XSD_DATETYPE: &str = "http://www.w3.org/2001/XMLSchema#dateType";

// Predicate for fetching human readable comments about ontology terms.
const RDFS_COMMENT: &str = "http://www.w3.org/2000/01/rdf-schema#comment";

// Tracker uses this class to represent files it knows about.  When the SPARQL
// query indicates that the resource is an instance of this class we adapt the
// UI accordingly.
const FILEDATAOBJECT: &str = "http://tracker.api.gnome.org/ontology/v3/nfo#FileDataObject";

#[derive(Clone, Default)]
/// Representation of a single row in the main grid.  Each row shows a
/// predicate/value pair both in a human readable form and in the raw form
/// returned by Tracker.  The `display_*` fields contain strings presented to the
/// user while the `native_*` fields hold the unmodified data that can be copied
/// to the clipboard.
struct TableRow {
    /// Localized predicate label shown to the user.
    display_predicate: String,
    /// URI form of the predicate as returned by Tracker.
    native_predicate: String,
    /// Value shown in the UI (may be formatted or shortened).
    display_value: String,
    /// Raw value from Tracker without any formatting.
    native_value: String,
}

/// Entry point of the application.  Parsing of command line arguments happens
/// here and we set up the GTK `Application` object.  This function is small but
/// it orchestrates the entire flow of the program.
fn main() {
    // Collect command line arguments excluding the program name.  `env::args()`
    // yields an iterator of OsString which we convert to owned `String`s for
    // convenience.
    let mut args: Vec<String> = env::args().skip(1).collect();

    // If the user requests help we simply print usage information and exit
    // without creating any GTK windows.  This keeps the CLI fast and scriptable.
    if args.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!("{}", USAGE);
        return;
    }

    // Options are parsed manually.  `--uri` indicates that the argument is a
    // literal URI and should not be converted from a filesystem path.  The
    // `--debug` flag enables additional debug output on stderr.
    let mut raw_uri = false;
    let mut debug_flag = false;

    // Pull any options from the front of the argument list.  This allows the
    // program to be called like `file-information -d myfile.txt` where the file
    // path comes last.
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

    // Create the GTK application.  We request `HANDLES_COMMAND_LINE` so that our
    // command line parsing callback is triggered even if another instance is
    // already running.
    let app = Application::builder()
        .application_id(APP_ID)
        .flags(ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    // We handle command line invocation so that opening a file from the command
    // line or from a file manager behaves the same.  This callback runs before
    // the application is fully started.
    app.connect_command_line(move |app, cmd_line| {
        let argv = cmd_line.arguments();
        // Collect the arguments provided by the shell.  The first value is the
        // program name which we skip.
        let inputs: Vec<String> = argv
            .iter()
            .skip(1)
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        // Honour `--help` when launched via the command line interface of an
        // existing instance.
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
        // After stripping options, the next argument is the target file or URI
        // that we should display.  If no such argument exists we print the usage
        // message.
        if let Some(id) = items.first() {
            let uri = if raw {
                id.clone()
            } else {
                // Convert filesystem path to URI when --uri was not given.
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

    // When the application is launched without arguments we do nothing here; a
    // window will only be shown once `connect_command_line` or `connect_open`
    // triggers `build_ui`.
    app.connect_activate(|_| {});

    // Support opening files via desktop integration.  The `GApplication::open`
    // signal delivers URIs of files the user wants to inspect.
    app.connect_open(move |app, files, _| {
        if let Some(file) = files.first() {
            build_ui(app, file.uri().to_string(), debug_flag);
        }
    });

    app.run();
}

/// Construct and present the main application window for the given `uri`.
/// The `debug` flag controls verbose logging of Tracker operations.  Most of
/// the widget hierarchy is assembled here.
fn build_ui(app: &Application, uri: String, debug: bool) {
    let window = ApplicationWindow::builder()
        .application(app)
        .default_width(590)
        .default_height(400)
        .title("File Information")
        .build();

    add_common_actions(&window);

    let provider = CssProvider::new();
    // Small embedded CSS snippet to style our grid and text views.  This keeps
    // the UI tidy without needing an external stylesheet.
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
    // Apply the CSS provider to the default display so the styles take effect
    // application-wide.
    if let Some(display) = Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    // Header bar shown at the top of the window.  We include standard close
    // buttons so the window integrates nicely with the desktop environment.
    let header = HeaderBar::new();
    header.set_show_end_title_buttons(true);

    // The header displays the title of the view.  It starts with a "Loading…"
    // label which will be replaced once the SPARQL query finishes.
    let header_label = Label::new(Some("Loading…"));
    header.set_title_widget(Some(&header_label));

    // Main grid used to display predicate/value pairs.  We avoid homogeneous
    // columns so that long values can consume the remaining space.
    let grid = Grid::builder()
        .column_homogeneous(false)
        .hexpand(true)
        .vexpand(true)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Fill)
        .build();
    grid.set_widget_name("data-grid");

    // The viewport ensures that the grid can be scrolled while keeping the CSS
    // styling simple.
    let viewport = gtk::Viewport::builder()
        .scroll_to_focus(false)
        .child(&grid)
        .build();

    // Put everything inside a scrolled window so large sets of metadata remain
    // navigable.
    let scroll = gtk::ScrolledWindow::builder()
        .min_content_width(590)
        .min_content_height(400)
        .child(&viewport)
        .build();

    // Adwaita's ToolbarView lets us stack a header and optional bottom bar on
    // top of a scrolled content area.
    let toolbar = ToolbarView::new();
    toolbar.add_top_bar(&header);

    // Store the table rows so that we can export them to CSV later.
    let table_data: Rc<RefCell<Vec<TableRow>>> = Rc::new(RefCell::new(Vec::new()));

    // Simple bottom bar buttons for closing the window and copying/exporting
    // data.  Each button gets its own handler.
    let close_button = Button::with_label("Close");
    let win_clone = window.clone();
    close_button.connect_clicked(move |_| {
        win_clone.close();
    });

    let copy_button = Button::with_label("Copy");
    let data_clone = table_data.clone();
    // Export the collected table rows as CSV and place the text on the
    // clipboard so it can be pasted elsewhere.
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
    // Allow the user to open the current URI with the system default handler.
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
    let debug_clone = debug;
    // Show a secondary window listing incoming references to the current URI.
    backlinks_button.connect_clicked(move |_| {
        show_backlinks_window(&app_clone, &win_parent, uri_bl.clone(), debug_clone);
    });

    // Pack the buttons into a horizontal box that forms the window's bottom
    // bar. Spacing and margins keep the layout pleasant regardless of language
    // direction.
    let bottom_box = GtkBox::new(Orientation::Horizontal, 0);
    bottom_box.set_spacing(5);
    bottom_box.set_halign(gtk::Align::End);
    bottom_box.set_margin_start(6);
    bottom_box.set_margin_end(6);
    bottom_box.set_margin_top(6);
    bottom_box.set_margin_bottom(6);
    bottom_box.append(&backlinks_button);
    bottom_box.append(&copy_button);
    // Only show the "Open" button if the URI has a registered handler.  For
    // example, some custom schemes might not be supported.
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

    // Kick off the asynchronous SPARQL query.  We use `spawn_local` so the
    // future runs on the main event loop without blocking the UI.
    glib::MainContext::default().spawn_local(async move {
        let (is_file_data_object, rows) =
            populate_grid(&app_clone, &window_clone, &grid_clone, &uri_clone, debug).await;
        let row_count = rows.len().saturating_sub(1);
        data_clone.borrow_mut().clear();
        data_clone.borrow_mut().extend(rows);

        header_clone.set_text(if is_file_data_object {
            "File Information"
        } else {
            "Node Information"
        });

        if debug {
            // When debugging we measure how long it takes until the results are
            // actually painted on screen.  The frame clock lets us run a
            // callback right after the next draw.
            if let Some(clock) = grid_clone.frame_clock() {
                use std::cell::RefCell;
                use gdk4::FrameClockPhase;

                // We disconnect the signal after running once to avoid leaking
                // handlers when multiple queries are performed.
                let handler: Rc<RefCell<Option<glib::SignalHandlerId>>> =
                    Rc::new(RefCell::new(None));
                let handler_clone = handler.clone();
                let id = clock.connect_after_paint(move |clk| {
                    if let Some(h) = handler_clone.borrow_mut().take() {
                        clk.disconnect(h);
                    }
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

/// Run a SPARQL query for `uri` and populate the provided `Grid` widget with
/// the results.  Returns whether the node represents a `FileDataObject` as well
/// as the vector of rows that were added to the grid.
async fn populate_grid(
    app: &Application,
    window: &ApplicationWindow,
    grid: &Grid,
    uri: &str,
    debug: bool,
) -> (bool, Vec<TableRow>) {
    // Remove any existing rows so the grid can be reused when navigating
    // between nodes.
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }
    if debug {
        eprintln!("Fetching backlinks for {uri}");
    }

    let mut rows_vec = Vec::new();

    // The first row always shows the URI of the node itself for quick copying
    // and reference.
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
    // Connect to the Tracker Miner on the session bus.  This is where metadata
    // for files is indexed.  Failure here usually means Tracker is not running
    // or D-Bus is misconfigured.
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

    // Build a SPARQL query that retrieves all predicate/value pairs for the
    // given resource and also returns the datatype for each value.
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

    // We collect results into a map so that multiple objects for the same
    // predicate are grouped together. `order` preserves the original ordering
    // of predicates from the query results.
    let mut order = Vec::new();
    let mut map: HashMap<String, Vec<(String, String)>> = HashMap::new();

    // Some UI text changes depending on whether the node is a tracked file or
    // an arbitrary resource.  We detect this by checking for the
    // `nfo:FileDataObject` type while iterating the results.
    let mut is_file_data_object = false;

    // Iterate over the query cursor asynchronously.  Each row contains the
    // predicate URI, object string and datatype URI if any.
    while cursor.next_future().await.unwrap_or(false) {
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
                    // When the user clicks on a predicate label we lazily
                    // query Tracker for any `rdfs:comment` attached to that
                    // predicate and show it as a tooltip.
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

                // The widget used to display the value depends on whether the
                // value looks like a URI or contains newlines.
                let widget: gtk::Widget = if dtype.is_empty() {
                    let lbl_link = Label::new(None);
                    let escaped = glib::markup_escape_text(obj);
                    lbl_link.set_markup(&format!("<a href=\"{0}\">{0}</a>", escaped));
                    lbl_link.set_halign(gtk::Align::Start);
                    lbl_link.set_margin_start(6);
                    lbl_link.set_margin_top(4);
                    lbl_link.set_margin_bottom(4);

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
                    // Multiline literals are shown in a readonly TextView so
                    // line wrapping behaves nicely.
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
    if debug {
        eprintln!(
            "DEBUG: query returned rows={} file_data={}",
            rows_vec.len() - 1,
            is_file_data_object
        );
    }
    (is_file_data_object, rows_vec)
}

/// Convert a predicate or URI fragment into a human readable label.  This is a
/// best effort heuristic that splits camel case words and capitalizes them.
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

/// Format a literal value for display based on its datatype.
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

/// Truncate `s` to at most `max_chars` Unicode characters, appending an
/// ellipsis if truncation occurred.
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

/// Very small helper that tries to parse the string as a URI.  It is used to
/// decide whether clicking should open another view or simply display text.
fn looks_like_uri(s: &str) -> bool {
    Url::parse(s).is_ok()
}

/// Check whether there is an application capable of opening the given URI.
/// Returns `Ok(())` if a handler exists or an error string describing the
/// problem otherwise.
fn uri_has_handler(uri: &str) -> Result<(), String> {
    if let Ok(url) = Url::parse(uri) {
        if url.scheme() == "file" {
            // For file URIs we look up the MIME type and check if there is a
            // default application registered for it.
            if let Ok(path) = url.to_file_path() {
                if let Some(p) = path.to_str() {
                    let (mime, _) = gio::content_type_guess(Some(p), b"");
                    if gio::AppInfo::default_for_type(&mime, false).is_none() {
                        return Err(format!("No application available for type \"{}\".", mime));
                    }
                }
            }
        } else if gio::AppInfo::default_for_uri_scheme(url.scheme()).is_none() {
            // Non-file URIs are checked via the scheme.  If no handler exists,
            // we return an error message that can be shown to the user.
            return Err(format!(
                "No application available for scheme \"{}\".",
                url.scheme()
            ));
        }
    }
    Ok(())
}

/// Install a handful of `Gio::SimpleAction` objects on the given window.  These
/// actions implement clipboard copy functionality and opening URIs with the
/// default system handler.
fn add_common_actions(window: &ApplicationWindow) {
    // `copy-displayed-value` copies exactly what the user sees in the UI.
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

    // `copy-native-value` copies the raw value returned by Tracker.
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
    // `open-uri` asks the desktop to launch the default handler for a URI.
    let open_uri_action = gio::SimpleAction::new("open-uri", Some(&VariantTy::STRING));
    open_uri_action.connect_activate(move |_action, param| {
        if let Some(v) = param {
            if let Some(uri) = v.str() {
                // In case launching fails we show an informative dialog.
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
}

/// Attach a context menu to `widget` that offers to copy the displayed value,
/// the native value and, when appropriate, to open the URI externally.
fn add_copy_menu<W>(widget: &W, displayed: &str, native: &str, disp_label: &str, nat_label: &str)
where
    W: IsA<gtk::Widget> + Clone + 'static,
{
    // Use a right-click gesture to trigger the popover menu.
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

        // Offer an "Open Externally" item only when the value looks like a URI
        // and a handler exists.
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

/// Create a secondary window displaying all subjects that reference the given
/// `uri`.  This is opened when the user clicks the "Backlinks" button.
fn show_backlinks_window(app: &Application, parent: &ApplicationWindow, uri: String, debug: bool) {
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

    glib::MainContext::default().spawn_local(async move {
        populate_backlinks_grid(&app_clone, &window_clone, &grid_clone, &uri_clone, debug_clone).await;
    });
}

/// Populate the backlinks grid with SPARQL query results.  Each row lists a
/// subject and predicate that point to the current resource.
async fn populate_backlinks_grid(app: &Application, window: &ApplicationWindow, grid: &Grid, uri: &str, debug: bool) {
    // Clear any previous results from the grid.
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }

    // Connect to Tracker so we can query who references the given URI.
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

    // Query for all subjects and predicates pointing at our node.
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
    // Each row of the result contains a subject and predicate referencing the
    // original URI.  We display these one per row.
    while cursor.next_future().await.unwrap_or(false) {
        let subj = cursor.string(0).unwrap_or_default().to_string();
        let pred = cursor.string(1).unwrap_or_default().to_string();

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
        eprintln!("Backlinks query returned {row} rows");
    }
}

/// Fetch the `rdfs:comment` for a predicate URI.  Returns `None` if no comment
/// could be retrieved.
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
