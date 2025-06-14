use adw::prelude::*;
use clap::Parser;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use tracker::prelude::*;

mod options;

const APP_ID: &str = "com.example.DesktopFileInformation";

const TOOLTIP_MAX_CHARS: usize = 80;
const COMMENT_TOOLTIP_MAX_CHARS: usize = TOOLTIP_MAX_CHARS * 3;

const XSD_DATETYPE: &str = "http://www.w3.org/2001/XMLSchema#dateType";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const RDFS_COMMENT: &str = "http://www.w3.org/2000/01/rdf-schema#comment";
const NIE_INTERPRETED_AS: &str = "http://tracker.api.gnome.org/ontology/v3/nie#interpretedAs";
const NIE_MIME_TYPE: &str = "http://tracker.api.gnome.org/ontology/v3/nie#mimeType";
const FILEDATAOBJECT: &str = "http://tracker.api.gnome.org/ontology/v3/nfo#FileDataObject";

#[derive(Clone, Default)]
struct TableRow {
    display_predicate: String,
    native_predicate: String,
    display_value: String,
    native_value: String,
}

/// Entry point. Parses command-line arguments and sets up the main `adw::Application` instance.
///
/// Supported command-line flags:
/// * `-h` / `--help` - only print usage help string and exit.
/// * `-u` / `--uri`  - interpret the provided argument as a URI rather than a filesystem path.
/// * `-d` / `--debug` - print additional diagnostic information to stderr.
fn main() {
    // Create a new `adw::Application` instance with a specific application ID and set its launch flags.
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(
            // NON_UNIQUE allows multiple instances,
            // HANDLES_COMMAND_LINE and HANDLES_OPEN let us manage command-line arguments and file open events.
            gio::ApplicationFlags::NON_UNIQUE
                | gio::ApplicationFlags::HANDLES_COMMAND_LINE
                | gio::ApplicationFlags::HANDLES_OPEN,
        )
        .build();

    // Register a handler for command-line invocation of the app (when started from terminal or by opening files).
    app.connect_command_line(|app, cmd_line| {
        let argv = cmd_line.arguments();
        // Parse command-line arguments using clap
        let opts = match options::Options::try_parse_from(argv) {
            Ok(c) => c,
            Err(e) => {
                e.print().expect("failed to write clap error");
                return e.exit_code();
            }
        };

        env_logger::Builder::new()
            .filter_level(if opts.debug {
                log::LevelFilter::Debug
            } else {
                log::LevelFilter::Warn
            })
            .init();

        let uri = if opts.uri {
            opts.item.clone()
        } else {
            gio::File::for_path(&opts.item).uri().to_string()
        };

        app.activate();
        open_subject_window(app, uri, opts.debug);
        0
    });

    // Register a handler for when files are opened by the system with the app (e.g., double-click
    // in file manager).
    app.connect_open(|app, files, _| {
        // If at least one file is present, build the UI for it.
        if let Some(file) = files.first() {
            open_subject_window(app, file.uri().to_string(), false);
        }
    });

    // Register a no-op handler for application activation (to satisfy GTK's requirements).
    app.connect_activate(|_| {});

    // Start running the application main loop. This function will not return until the app exits.
    app.run();
}

/// Builds and presents the main window UI for a given URI.
///
/// This function creates and configures the main GTK application window, sets up styling,
/// assembles all the widgets, registers callbacks, and asynchronously populates the UI
/// with information about the file or node referenced by the `uri` argument.
///
/// # Arguments
/// * `app` - The application instance, used for context and for spawning additional windows.
/// * `uri` - The URI (can be a file path or another type) to display information about.
/// * `debug` - If true, prints additional diagnostic info to stderr.
fn open_subject_window(app: &adw::Application, uri: String, debug: bool) {
    // Create the main application window with specified size and title.
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .default_width(590)
        .default_height(400)
        .title("File Information")
        .build();

    // Add common actions (i.e., copy to clipboard, open URI) for context menus in this window.
    add_common_actions(&window);

    // Prepare a CSS provider and style the grid and its children.
    let provider = gtk::CssProvider::new();
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
    // Apply CSS styling globally to all GTK widgets for the current display.
    if let Some(display) = gdk4::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    // Create the header bar (top bar) with a label that displays the title or loading state.
    let header = adw::HeaderBar::new();
    header.set_show_end_title_buttons(true);

    let header_label = gtk::Label::new(Some("Loading…"));
    header.set_title_widget(Some(&header_label));

    // Construct a grid that will display all the file/node information in two columns.
    let grid = gtk::Grid::builder()
        .column_homogeneous(false)
        .hexpand(true)
        .vexpand(true)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Fill)
        .build();
    grid.set_widget_name("data-grid");

    // Add the grid inside a viewport, which allows for scrolling if content is large.
    let viewport = gtk::Viewport::builder()
        .scroll_to_focus(false)
        .child(&grid)
        .build();

    // Put the viewport into a scrollable window with minimum dimensions.
    let scroll = gtk::ScrolledWindow::builder()
        .min_content_width(590)
        .min_content_height(400)
        .child(&viewport)
        .build();

    // Create a custom toolbar to host header and bottom bar.
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);

    // Store table data (file/node attributes) in a shared, mutable reference for use by callbacks.
    let table_data: Rc<RefCell<Vec<TableRow>>> = Rc::new(RefCell::new(Vec::new()));

    // ----- Bottom bar with buttons -----

    // "Close" button: closes the window when clicked.
    let close_button = gtk::Button::with_label("Close");
    let win_clone = window.clone();
    close_button.connect_clicked(move |_| {
        win_clone.close();
    });

    // "Copy" button: copies the displayed table as CSV to the clipboard.
    let copy_button = gtk::Button::with_label("Copy");
    let data_clone = table_data.clone();
    copy_button.connect_clicked(move |_| {
        let rows = data_clone.borrow();
        // Prepare a CSV writer and add headers.
        let mut wtr = csv::WriterBuilder::new()
            .has_headers(true)
            .from_writer(vec![]);
        let _ = wtr.write_record([
            "Display Predicate",
            "Native Predicate",
            "Display Value",
            "Native Value",
        ]);
        // Write each row from the table to CSV.
        for r in rows.iter() {
            let _ = wtr.write_record([
                &r.display_predicate,
                &r.native_predicate,
                &r.display_value,
                &r.native_value,
            ]);
        }
        // Convert CSV to UTF-8 string and copy to clipboard if successful.
        if let Ok(data) = String::from_utf8(wtr.into_inner().unwrap_or_default()) {
            if let Some(display) = gdk4::Display::default() {
                display.clipboard().set_text(&data);
            }
        }
    });

    // "Open" button: triggers the open-uri action using the window and the current URI.
    let open_button = gtk::Button::with_label("Open");
    let win_for_action = window.clone();
    let uri_clone = uri.clone();
    open_button.connect_clicked(move |_| {
        gio::prelude::ActionGroupExt::activate_action(
            &win_for_action,
            "open-uri",
            Some(&glib::Variant::from(uri_clone.as_str())),
        );
    });

    // "Backlinks" button: opens a window showing referencing nodes.
    let backlinks_button = gtk::Button::with_label("Backlinks");
    let app_clone = app.clone();
    let win_parent = window.clone();
    let uri_bl = uri.clone();
    let debug_clone = debug;
    backlinks_button.connect_clicked(move |_| {
        open_object_window(&app_clone, &win_parent, uri_bl.clone(), debug_clone);
    });

    // Arrange all bottom bar buttons in a horizontal box, aligned to the end.
    let bottom_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bottom_box.set_spacing(5);
    bottom_box.set_halign(gtk::Align::End);
    bottom_box.set_margin_start(6);
    bottom_box.set_margin_end(6);
    bottom_box.set_margin_top(6);
    bottom_box.set_margin_bottom(6);
    bottom_box.append(&backlinks_button);
    bottom_box.append(&copy_button);
    // Only show the "Open" button if the URI has a registered external handler.
    if uri_has_handler(&uri).is_ok() {
        bottom_box.append(&open_button);
    }
    bottom_box.append(&close_button);
    toolbar.add_bottom_bar(&bottom_box);

    // Insert the scrollable grid as the main content in the window via the toolbar.
    toolbar.set_content(Some(&scroll));
    window.set_content(Some(&toolbar));
    // Present the window (show it on screen).
    window.present();

    // ---- Asynchronous section to populate the grid with file/node info ----
    let app_clone = app.clone();
    let window_clone = window.clone();
    let grid_clone = grid.clone();
    let header_clone = header_label.clone();
    let data_clone = table_data.clone();
    let uri_clone = uri.clone();

    // Spawn an async block on the GTK main context.
    glib::MainContext::default().spawn_local(async move {
        // Query data and fill the grid; returns type info and the rows.
        let (is_file_data_object, rows) =
            populate_grid(&app_clone, &window_clone, &grid_clone, &uri_clone, debug).await;
        let row_count = rows.len().saturating_sub(1);
        // Update the table data for other parts of the UI (e.g., copy button).
        data_clone.borrow_mut().clear();
        data_clone.borrow_mut().extend(rows);

        // Set the header label to reflect the object type.
        header_clone.set_text(if is_file_data_object {
            "File Information"
        } else {
            "Node Information"
        });

        // If debug is enabled, print diagnostics about results, but only immediately after the
        // grid has been fully painted, and therefore is ready for a screen capture.
        if debug {
            if let Some(clock) = grid_clone.frame_clock() {
                let handler: Rc<RefCell<Option<glib::SignalHandlerId>>> =
                    Rc::new(RefCell::new(None));
                let handler_clone = handler.clone();
                let id = clock.connect_after_paint(move |clk| {
                    if let Some(h) = handler_clone.borrow_mut().take() {
                        clk.disconnect(h);
                    }
                    log::debug!(
                        "DEBUG: results displayed rows={} file_data={}",
                        row_count,
                        is_file_data_object
                    );
                });
                *handler.borrow_mut() = Some(id);
                clock.request_phase(gdk4::FrameClockPhase::AFTER_PAINT);
            }
        }
    });
}

/// Adds actions for copying data to the clipboard and opening links externally such that these
/// actions can be added to context menus.
///
/// # Arguments
/// * `window` - The main application window to which the actions will be added.
fn add_common_actions(window: &adw::ApplicationWindow) {
    // ----- "Copy Value" Action -----
    // Create a new action named "copy-value" that accepts a single string argument.
    // This action allows copying arbitrary string data to the system clipboard.
    let copy_value = gio::SimpleAction::new("copy-value", Some(glib::VariantTy::STRING));

    // Register a handler for when the "copy-value" action is activated.
    // This closure receives the action object and the optional parameter (expected to be a string).
    copy_value.connect_activate(move |_action, param| {
        // Only proceed if a parameter was supplied.
        if let Some(v) = param {
            // Extract the string value from the parameter.
            if let Some(text) = v.str() {
                // Attempt to get the default display (may fail if no display is available).
                if let Some(display) = gdk4::Display::default() {
                    // Access the system clipboard for the display.
                    let clipboard = display.clipboard();
                    // Set the clipboard contents to the provided text.
                    clipboard.set_text(text);
                }
            }
        }
    });
    // Add the "copy-value" action to the window so it can be invoked from the UI or programmatically.
    window.add_action(&copy_value);

    // ----- "Open URI" Action -----
    // Prepare to create an action that attempts to open a URI using the system's default handler.
    // We clone the window so the action's closure can use it for dialog ownership.
    let win_for_uri = window.clone();
    // Create a new action named "open-uri" that takes a string argument (the URI to open).
    let open_uri_action = gio::SimpleAction::new("open-uri", Some(glib::VariantTy::STRING));

    // Register a handler for the "open-uri" action.
    open_uri_action.connect_activate(move |_action, param| {
        // Only proceed if a parameter (the URI) was supplied.
        if let Some(v) = param {
            if let Some(uri) = v.str() {
                // Define a helper function to show an informational dialog with an error message.
                // This will be used if the URI cannot be handled or if opening fails.
                let report = |msg: String| {
                    // Build a modal dialog attached to the main window with the error details.
                    let dialog = gtk::MessageDialog::builder()
                        .transient_for(&win_for_uri)
                        .modal(true)
                        .message_type(gtk::MessageType::Info)
                        .buttons(gtk::ButtonsType::Ok)
                        .text("Could not open URI")
                        .secondary_text(&msg)
                        .build();
                    // Close the dialog when any response is received (e.g., user clicks OK).
                    dialog.connect_response(|dlg, _| dlg.close());
                    dialog.show();
                };

                // First, check if there is a handler registered for this URI scheme/type.
                // If not, show a dialog to the user and exit early.
                if let Err(msg) = uri_has_handler(uri) {
                    report(msg);
                    return;
                }

                // Attempt to launch the URI using the system's default application.
                // If this fails (e.g., no handler, launch error), report the error to the user.
                if let Err(err) =
                    gio::AppInfo::launch_default_for_uri(uri, None::<&gio::AppLaunchContext>)
                {
                    report(err.to_string());
                }
            }
        }
    });
    // Add the "open-uri" action to the window for use by UI elements or other parts of the code.
    window.add_action(&open_uri_action);
}

/// Opens a new window displaying the backlinks (referencing nodes) for a given URI.
///
/// This function creates a secondary application window styled and sized similarly to the main window,
/// but focused on displaying "backlinks". It constructs the UI layout (header, grid, scrolling container,
/// close button, etc.), adds all necessary actions, and asynchronously populates the grid with
/// backlink data from Tracker.
///
/// # Arguments
/// * `app` - Reference to the main application instance.
/// * `parent` - The parent window to which this window will be transient (modal behavior).
/// * `uri` - The URI of the object for which to display backlinks.
/// * `debug` - If true, prints debug information during operation.
fn open_object_window(
    app: &adw::Application,
    parent: &adw::ApplicationWindow,
    uri: String,
    debug: bool,
) {
    // ---- Window Construction ----

    // Create a new GTK application window, sized and titled appropriately for backlinks.
    // The window is set as transient for its parent for correct stacking and modality.
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .transient_for(parent)
        .default_width(590)
        .default_height(400)
        .title("Backlinks")
        .build();

    // Add common actions (i.e., copy to clipboard, open URI) for context menus in this window.
    add_common_actions(&window);

    // ---- Header Bar ----

    // Construct a header bar (title bar) with a title label.
    let header = adw::HeaderBar::new();
    header.set_show_end_title_buttons(true);
    let header_label = gtk::Label::new(Some("Backlinks"));
    header.set_title_widget(Some(&header_label));

    // ---- Main Grid for Backlinks Data ----

    // Create a GTK grid widget to display backlink entries.
    let grid = gtk::Grid::builder()
        .column_homogeneous(false)
        .hexpand(true)
        .vexpand(true)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Fill)
        .build();
    grid.set_widget_name("data-grid"); // Set a name for styling via CSS.

    // Embed the grid inside a viewport for scroll support (handles large data).
    let viewport = gtk::Viewport::builder()
        .scroll_to_focus(false)
        .child(&grid)
        .build();

    // Wrap the viewport in a scrolled window, fixing the minimum content size.
    let scroll = gtk::ScrolledWindow::builder()
        .min_content_width(590)
        .min_content_height(400)
        .child(&viewport)
        .build();

    // ---- Toolbar and Bottom Bar ----

    // Create a toolbar to host the header and bottom bar.
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);

    // Create a "Close" button to allow the user to dismiss the window.
    let close_button = gtk::Button::with_label("Close");
    let win_clone = window.clone();
    close_button.connect_clicked(move |_| {
        win_clone.close();
    });

    // Layout the close button in a horizontal box, aligned to the end, with spacing and margins.
    let bottom_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bottom_box.set_spacing(5);
    bottom_box.set_halign(gtk::Align::End);
    bottom_box.set_margin_start(6);
    bottom_box.set_margin_end(6);
    bottom_box.set_margin_top(6);
    bottom_box.set_margin_bottom(6);
    bottom_box.append(&close_button);
    toolbar.add_bottom_bar(&bottom_box);

    // Set the scrolled grid as the main content of the toolbar, and set the toolbar as the window's content.
    toolbar.set_content(Some(&scroll));
    window.set_content(Some(&toolbar));

    // Present (show) the window to the user.
    window.present();

    // ---- Asynchronous Population of Backlinks Data ----

    // Clone references needed for the async block, since closures move their environment.
    let app_clone = app.clone();
    let window_clone = window.clone();
    let grid_clone = grid.clone();
    let uri_clone = uri.clone();
    let debug_clone = debug;

    // Spawn an asynchronous task in the main context to populate the backlinks grid.
    glib::MainContext::default().spawn_local(async move {
        populate_backlinks_grid(
            &app_clone,
            &window_clone,
            &grid_clone,
            &uri_clone,
            debug_clone,
        )
        .await;
    });
}

/// Asynchronously populates a GTK grid widget with backlinks—nodes that reference the given URI.
///
/// This function queries the Tracker database to find all subject-predicate pairs (?s ?p)
/// where the object is the provided URI (i.e., all triples pointing to this node),
/// then creates grid rows for each backlink, formatting and linking as appropriate.
///
/// # Arguments
/// * `app` - Reference to the application instance, used for UI actions.
/// * `window` - The parent window, used for modal dialogs.
/// * `grid` - The GTK grid to populate with backlink data.
/// * `uri` - The URI whose backlinks are to be listed.
/// * `debug` - If true, emits diagnostic output during execution.
async fn populate_backlinks_grid(
    app: &adw::Application,
    window: &adw::ApplicationWindow,
    grid: &gtk::Grid,
    uri: &str,
    debug: bool,
) {
    // ---- Clear Existing Grid Content ----
    // Remove all current children from the grid so we start with a blank slate.
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }

    // ---- Connect to Tracker and Handle Errors ----
    let conn = match create_store_connection() {
        Ok(c) => c,
        Err(err) => {
            // If connection fails, show an error dialog and return early.
            if debug {
                log::debug!("Failed to connect to Tracker: {err}");
            }
            let dialog = gtk::MessageDialog::builder()
                .transient_for(window)
                .modal(true)
                .message_type(gtk::MessageType::Error)
                .text("Failed to connect to Tracker")
                .secondary_text(format!("{err}"))
                .buttons(gtk::ButtonsType::Ok)
                .build();
            dialog.connect_response(|dlg, _| dlg.close());
            dialog.show();
            return;
        }
    };

    // ---- Prepare and Run the SPARQL Query ----
    // Query for all subject-predicate pairs where the object matches the given URI.
    let sparql = format!("SELECT DISTINCT ?s ?p WHERE {{ ?s ?p <{uri}> }}", uri = uri);
    if debug {
        log::debug!("Running SPARQL query: {sparql}");
    }
    let cursor = match conn.query_future(&sparql).await {
        Ok(c) => c,
        Err(err) => {
            // If query fails, show an error dialog and return early.
            if debug {
                log::debug!("SPARQL query error: {err}");
            }
            let dialog = gtk::MessageDialog::builder()
                .transient_for(window)
                .modal(true)
                .message_type(gtk::MessageType::Error)
                .text("SPARQL query error")
                .secondary_text(format!("{err}"))
                .buttons(gtk::ButtonsType::Ok)
                .build();
            dialog.connect_response(|dlg, _| dlg.close());
            dialog.show();
            return;
        }
    };

    // ---- Iterate Through Query Results and Populate the Grid ----
    let mut row = 0;
    while cursor.next_future().await.unwrap_or(false) {
        // Extract the subject and predicate from the current result row.
        let subj = cursor.string(0).unwrap_or_default().to_string();
        let pred = cursor.string(1).unwrap_or_default().to_string();

        // ---- Create a Widget for the Subject Node ----
        // If the subject looks like a URI, present it as a clickable link; otherwise, as plain text.
        let widget: gtk::Widget = if looks_like_uri(&subj) {
            let lbl_link = gtk::Label::new(None);
            let escaped = glib::markup_escape_text(&subj);
            lbl_link.set_markup(&format!("<a href=\"{0}\">{0}</a>", escaped));
            lbl_link.set_halign(gtk::Align::Start);
            lbl_link.set_margin_start(6);
            lbl_link.set_margin_top(4);
            lbl_link.set_margin_bottom(4);
            lbl_link.set_wrap(true);
            lbl_link.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            lbl_link.set_max_width_chars(80);

            // Make the link clickable: opens the subject in the UI.
            let app_clone = app.clone();
            let debug_clone = debug;
            lbl_link.connect_activate_link(move |_lbl, uri| {
                open_subject_window(&app_clone, uri.to_string(), debug_clone);
                glib::Propagation::Stop
            });

            // Add a context menu for copying values.
            add_copy_menu(
                &lbl_link,
                &subj,
                &subj,
                "Copy Displayed Value",
                "Copy Native Value",
            );

            lbl_link.upcast()
        } else {
            // For plain text subjects, use a regular label.
            let lbl_val = gtk::Label::new(Some(&subj));
            lbl_val.set_halign(gtk::Align::Start);
            lbl_val.set_margin_start(6);
            lbl_val.set_margin_top(4);
            lbl_val.set_margin_bottom(4);
            lbl_val.set_wrap(true);
            lbl_val.set_wrap_mode(gtk::pango::WrapMode::WordChar);
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

        // Set a tooltip on the subject widget for full value visibility.
        widget.set_tooltip_text(Some(&subj));
        // Attach the subject widget to the first column of the current row.
        grid.attach(&widget, 0, row, 1, 1);

        // ---- Create and Attach Predicate Label ----
        // Convert predicate URI to a friendly display label.
        let pred_label = friendly_label(&pred);
        let lbl_pred = gtk::Label::new(Some(&pred_label));
        lbl_pred.set_halign(gtk::Align::Start);
        lbl_pred.set_valign(gtk::Align::Start);
        lbl_pred.style_context().add_class("first-col");
        lbl_pred.set_tooltip_text(Some(&pred));
        lbl_pred.set_margin_start(6);
        lbl_pred.set_margin_top(4);
        lbl_pred.set_margin_bottom(4);

        // Add context menu for copying the predicate.
        add_copy_menu(
            &lbl_pred,
            &pred_label,
            &pred,
            "Copy Displayed Predicate",
            "Copy Native Predicate",
        );

        // Attach the predicate label to the second column of the current row.
        grid.attach(&lbl_pred, 1, row, 1, 1);

        // Move to the next row for the next result.
        row += 1;
    }

    // ---- Final Debug Output ----
    if debug {
        log::debug!("Backlinks query returned {row} rows");
    }
}

/// Determines whether the system has a registered application handler for a given URI.
///
/// This function inspects the URI's scheme (e.g., "file", "http") and checks whether
/// there is a suitable application available to open it. If not, it returns an error
/// with a human-readable message indicating the missing handler.
///
/// # Arguments
/// * `uri` - The URI string to check (may be a file path, web link, etc.).
///
/// # Returns
/// * `Ok(())` if a suitable handler exists for the URI's scheme or MIME type.
/// * `Err(String)` with a descriptive message if no handler is found.
fn uri_has_handler(uri: &str) -> Result<(), String> {
    // Attempt to parse the URI using the Url crate to inspect its components.
    if let Ok(url) = url::Url::parse(uri) {
        // If the scheme is "file", handle as a local file.
        if url.scheme() == "file" {
            // Try to convert the file URI to a platform-native path.
            if let Ok(path) = url.to_file_path() {
                if let Some(p) = path.to_str() {
                    // Attempt to determine the MIME type for the file.
                    // First, try to use the indexed content type; if not found, guess based on filename.
                    let mime = get_indexed_content_type(uri).unwrap_or_else(|| {
                        let (guess, _) = gio::content_type_guess(Some(p), b"");
                        guess.to_string()
                    });
                    // Check if there is a default application for this MIME type.
                    // If not, return an error indicating the missing handler.
                    if gio::AppInfo::default_for_type(&mime, false).is_none() {
                        return Err(format!("No application available for type \"{}\".", mime));
                    }
                }
            }
        }
        // For non-file URIs, check for a default application registered for the URI's scheme.
        else if gio::AppInfo::default_for_uri_scheme(url.scheme()).is_none() {
            return Err(format!(
                "No application available for scheme \"{}\".",
                url.scheme()
            ));
        }
    }
    // If all checks pass, a handler exists; return success.
    Ok(())
}

/// Creates a new connection to the Tracker store via D-Bus.
///
/// This helper wraps `tracker::SparqlConnection::bus_new` with the
/// fixed service name used throughout the application.
fn create_store_connection() -> Result<tracker::SparqlConnection, glib::Error> {
    tracker::SparqlConnection::bus_new("org.freedesktop.Tracker3.Miner.Files", None, None)
}

/// Queries the Tracker index for the MIME content type associated with a given URI, if available.
///
/// This function attempts to determine the indexed content type (MIME type) for a file or resource
/// by executing a SPARQL query against the Tracker database. If no content type can be found,
/// it returns `None`.
///
/// # Arguments
/// * `uri` - The URI of the file or resource whose content type should be queried.
///
/// # Returns
/// An `Option<String>` containing the MIME type (e.g., "application/pdf") if found, or `None` otherwise.
fn get_indexed_content_type(uri: &str) -> Option<String> {
    // Attempt to create a connection to the Tracker D-Bus service.
    // If the service is unavailable or the connection fails, return None immediately.
    let conn = create_store_connection().ok()?;

    // Prepare a SPARQL query to fetch the indexed content type for the given URI.
    // The query traverses from the file node to its "interpreted as" node, then retrieves its MIME type.
    let sparql = format!(
        "SELECT ?ct WHERE {{ <{uri}> <{interp}> ?o . ?o <{mime}> ?ct }} LIMIT 1",
        uri = uri,
        interp = NIE_INTERPRETED_AS,
        mime = NIE_MIME_TYPE
    );

    // Execute the SPARQL query on the Tracker service.
    // If the query fails, return None.
    let cursor = conn.query(&sparql, None::<&gio::Cancellable>).ok()?;

    // If there is at least one result row, handle that one row.
    if cursor.next(None::<&gio::Cancellable>).unwrap_or(false) {
        // Extract the first string result (expected to be the content type).
        let ct = cursor.string(0).unwrap_or_default().to_string();
        // If the content type is an empty string, treat as not found.
        if ct.is_empty() { None } else { Some(ct) }
    } else {
        // If the query returned no results, return None.
        None
    }
}

/// Populates a GTK grid widget with metadata and properties for a given URI,
/// querying Tracker and formatting the results as table rows.
///
/// This function performs a SPARQL query against the Tracker database for the
/// provided URI, then fills the given `grid` with the results, row by row,
/// and returns both a flag indicating if the URI represents a file data object
/// and a vector of structured table rows for use elsewhere in the UI.
///
/// # Arguments
/// * `app` - Reference to the main application instance (used for launching sub-UIs).
/// * `window` - The application window owning the grid (used for modal dialogs).
/// * `grid` - The GTK grid widget to populate with result rows.
/// * `uri` - The URI to inspect and display information about.
/// * `debug` - If true, prints diagnostic information to stderr during processing.
///
/// # Returns
/// * `(bool, Vec<TableRow>)` - A tuple where the boolean indicates whether the URI
///   is a file data object, and the vector contains the table rows to display.
async fn populate_grid(
    app: &adw::Application,
    window: &adw::ApplicationWindow,
    grid: &gtk::Grid,
    uri: &str,
    debug: bool,
) -> (bool, Vec<TableRow>) {
    // Clear any existing children from the grid to prepare for new content.
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }

    // If debugging is enabled, print which URI we are processing.
    if debug {
        log::debug!("Fetching backlinks for {uri}");
    }

    // Initialize a vector to collect all the table rows we generate.
    let mut rows_vec = Vec::new();

    // ---- Add the Identifier Row ----

    // Create and style a label for the "Identifier" predicate.
    let id_label = gtk::Label::new(Some("Identifier"));
    id_label.set_halign(gtk::Align::Start);
    id_label.set_valign(gtk::Align::Start);
    id_label.style_context().add_class("first-col");
    id_label.set_margin_start(6);
    id_label.set_margin_top(4);
    id_label.set_margin_bottom(4);

    // Create a label displaying the URI itself, with word-wrapping and styling.
    let uri_label = gtk::Label::new(Some(uri));
    uri_label.set_halign(gtk::Align::Start);
    uri_label.set_margin_start(6);
    uri_label.set_margin_top(4);
    uri_label.set_margin_bottom(4);
    uri_label.set_wrap(true);
    uri_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    uri_label.set_max_width_chars(80);

    // Attach context menu actions for copying the displayed and native values from the label.
    add_copy_menu(
        &uri_label,
        uri,
        uri,
        "Copy Displayed Value",
        "Copy Native Value",
    );

    // Add a tooltip to the URI label, shortening the text if needed.
    let tooltip_text = ellipsize(uri, TOOLTIP_MAX_CHARS);
    uri_label.set_tooltip_text(Some(&tooltip_text));

    // Attach the labels to the first row of the grid.
    grid.attach(&id_label, 0, 0, 1, 1);
    grid.attach(&uri_label, 1, 0, 1, 1);

    // Record this as the first table row for later copying/export.
    rows_vec.push(TableRow {
        display_predicate: "Identifier".to_string(),
        native_predicate: "Identifier".to_string(),
        display_value: uri.to_string(),
        native_value: uri.to_string(),
    });

    // ---- Query Tracker for Additional Metadata ----

    if debug {
        log::debug!("Connecting to Tracker database for metadata…");
    }
    // Try to connect to the Tracker D-Bus service for SPARQL queries.
    let conn = match create_store_connection() {
        Ok(c) => c,
        Err(err) => {
            // On error, show an error dialog and return empty result.
            if debug {
                log::debug!("Failed to connect to Tracker: {err}");
            }
            let dialog = gtk::MessageDialog::builder()
                .transient_for(window)
                .modal(true)
                .message_type(gtk::MessageType::Error)
                .text("Failed to connect to Tracker")
                .secondary_text(format!("{err}"))
                .buttons(gtk::ButtonsType::Ok)
                .build();
            dialog.connect_response(|dlg, _| dlg.close());
            dialog.show();
            return (false, Vec::new());
        }
    };

    // Prepare a SPARQL query to get all direct predicates and objects for this node.
    let sparql = format!(
        r#"
        SELECT DISTINCT ?pred ?obj (DATATYPE(?obj) AS ?dtype) WHERE {{
            <{uri}> ?pred ?obj .
        }}
    "#,
        uri = uri
    );
    if debug {
        log::debug!("Running SPARQL query: {sparql}");
    }
    // Run the query asynchronously; handle errors by reporting them to the user.
    let cursor = match conn.query_future(&sparql).await {
        Ok(c) => c,
        Err(err) => {
            if debug {
                log::debug!("SPARQL query error: {err}");
            }
            let dialog = gtk::MessageDialog::builder()
                .transient_for(window)
                .modal(true)
                .message_type(gtk::MessageType::Error)
                .text("SPARQL query error")
                .secondary_text(format!("{err}"))
                .buttons(gtk::ButtonsType::Ok)
                .build();
            dialog.connect_response(|dlg, _| dlg.close());
            dialog.show();
            return (false, Vec::new());
        }
    };

    // ---- Collect Results Into an Ordered Map ----

    // Preserve the order in which predicates appear for display.
    let mut order = Vec::new();
    // Map each predicate to a list of (object value, datatype) pairs.
    let mut map: HashMap<String, Vec<(String, String)>> = HashMap::new();

    // Flag indicating if the node is a file data object.
    let mut is_file_data_object = false;

    // Iterate through all rows of the SPARQL result set.
    while cursor.next_future().await.unwrap_or(false) {
        let pred = cursor.string(0).unwrap_or_default().to_string();
        let obj = cursor.string(1).unwrap_or_default().to_string();
        let dtype = cursor.string(2).unwrap_or_default().to_string();

        // Track order of predicates as we see them.
        if !map.contains_key(&pred) {
            order.push(pred.clone());
            map.insert(pred.clone(), Vec::new());
        }
        map.get_mut(&pred)
            .unwrap()
            .push((obj.clone(), dtype.clone()));

        // Check for a special RDF type indicating whether the node is a file data object.
        if pred == RDF_TYPE && obj == FILEDATAOBJECT {
            is_file_data_object = true;
        }
    }

    // ---- Build Grid Rows for Each Predicate and Object ----

    let mut row = 1; // Start from row 1 (row 0 is the identifier)
    for pred in order {
        if let Some(entries) = map.get(&pred) {
            // Convert the raw predicate URI to a user-friendly label.
            let label_text = friendly_label(&pred);

            for (i, (obj, dtype)) in entries.iter().enumerate() {
                // Only add the predicate label in the first row for multi-valued predicates.
                if i == 0 {
                    let lbl_key = gtk::Label::new(Some(&label_text));
                    lbl_key.set_halign(gtk::Align::Start);
                    lbl_key.set_valign(gtk::Align::Start);
                    lbl_key.style_context().add_class("first-col");
                    // Initially, use the raw native predicate URI as tooltip text.
                    lbl_key.set_tooltip_text(Some(&pred));
                    lbl_key.set_margin_start(6);
                    lbl_key.set_margin_top(4);
                    lbl_key.set_margin_bottom(4);

                    // Add context menu for copying predicate names.
                    add_copy_menu(
                        &lbl_key,
                        &label_text,
                        &pred,
                        "Copy Displayed Predicate",
                        "Copy Native Predicate",
                    );

                    // If user clicks the predicate label, fetch description/comment for the
                    // predicate from Tracker and update the tooltip to present it.
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

                    // If mouse pointer leaves the predicate label, restore the original tooltip
                    // text.
                    let lbl_key_leave = lbl_key.clone();
                    let pred_leave = pred.clone();
                    let motion = gtk::EventControllerMotion::new();
                    motion.connect_leave(move |_| {
                        lbl_key_leave.set_tooltip_text(Some(&pred_leave));
                    });
                    lbl_key.add_controller(motion);

                    // Attach the predicate label to the grid.
                    grid.attach(&lbl_key, 0, row, 1, 1);
                }

                // Displayed value uses a formatter if we know the datatype, else show raw object.
                let displayed_str = if dtype.is_empty() {
                    obj.clone()
                } else {
                    friendly_value(obj, dtype)
                };
                let native_str = obj.clone();

                // Choose widget based on the object value datatype and contents.
                let widget: gtk::Widget = if dtype.is_empty() {
                    // Untyped object values are assumed to be URIs representing RDF nodes that
                    // should be rendered as links.
                    let lbl_link = gtk::Label::new(None);
                    let escaped = glib::markup_escape_text(obj);
                    lbl_link.set_markup(&format!("<a href=\"{0}\">{0}</a>", escaped));
                    lbl_link.set_halign(gtk::Align::Start);
                    lbl_link.set_margin_start(6);
                    lbl_link.set_margin_top(4);
                    lbl_link.set_margin_bottom(4);

                    // If such a link is clicked, a new subject window should be opened for the
                    // node in question.
                    let app_clone = app.clone();
                    let debug_clone = debug;
                    lbl_link.connect_activate_link(move |_lbl, uri| {
                        open_subject_window(&app_clone, uri.to_string(), debug_clone);
                        glib::Propagation::Stop
                    });

                    lbl_link.set_wrap(true);
                    lbl_link.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                    lbl_link.set_max_width_chars(80);

                    // Add context menu for copying object values.
                    add_copy_menu(
                        &lbl_link,
                        &displayed_str,
                        &native_str,
                        "Copy Displayed Value",
                        "Copy Native Value",
                    );

                    lbl_link.upcast()
                } else if obj.contains('\n') {
                    // For typed multi-line values, display in a non-editable text view.
                    let txt = gtk::TextView::new();
                    txt.set_editable(false);
                    txt.set_cursor_visible(false);
                    txt.style_context().add_class("bordered");
                    txt.set_wrap_mode(gtk::WrapMode::Word);
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
                    // For all other typed values, display in a standard label.
                    let lbl_val = gtk::Label::new(Some(&displayed_str));
                    lbl_val.set_halign(gtk::Align::Start);
                    lbl_val.set_margin_start(6);
                    lbl_val.set_margin_top(4);
                    lbl_val.set_margin_bottom(4);
                    lbl_val.set_wrap(true);
                    lbl_val.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                    lbl_val.set_max_width_chars(80);

                    add_copy_menu(
                        &lbl_val,
                        &displayed_str,
                        &native_str,
                        "Copy Displayed Value",
                        "Copy Native Value",
                    );
                    lbl_val.upcast()
                };

                // Set a tooltip for the native (raw) value.
                let tooltip_text = ellipsize(&native_str, TOOLTIP_MAX_CHARS);
                widget.set_tooltip_text(Some(&tooltip_text));

                // Attach the value widget to the grid.
                grid.attach(&widget, 1, row, 1, 1);

                // Record the row for exporting or copying later.
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

    // Print summary of query results if debugging.
    if debug {
        log::debug!(
            "query returned rows={} file_data={}",
            rows_vec.len() - 1,
            is_file_data_object
        );
    }

    // Return both the file data object flag and all collected rows.
    (is_file_data_object, rows_vec)
}

/// Attaches a right-click context menu to a GTK widget for copying its displayed and native values,
/// and optionally for opening URIs externally.
///
/// When the user right-clicks on the widget, a popover menu appears offering:
///   - "Copy Displayed Value": Copies the value as displayed in the UI to the clipboard.
///   - "Copy Native Value": Copies the raw or underlying value to the clipboard.
///   - "Open Externally" (only if the native value is a URI and the system has a handler): Opens the URI using the system's default handler.
///
/// # Arguments
/// * `widget` - The widget to which the context menu will be attached. Must implement `gtk::Widget`.
/// * `displayed` - The string as shown in the UI (user-facing, possibly formatted).
/// * `native` - The raw value, e.g., the underlying URI or identifier.
/// * `disp_label` - Label for the displayed value copy menu item (e.g., "Copy Displayed Value").
/// * `nat_label` - Label for the native value copy menu item (e.g., "Copy Native Value").
///
/// This function is generic and can be attached to any widget that implements `IsA<gtk::Widget>`.
fn add_copy_menu<W>(widget: &W, displayed: &str, native: &str, disp_label: &str, nat_label: &str)
where
    W: IsA<gtk::Widget> + Clone + 'static,
{
    // Create a GestureClick controller to detect right mouse button (button 3) presses.
    let gesture = gtk::GestureClick::new();
    gesture.set_button(3); // Only trigger on right-click.
    gesture.set_exclusive(true); // Ensure this gesture takes precedence over others.
    gesture.set_propagation_phase(gtk::PropagationPhase::Capture); // Capture events early.

    // Clone the data needed for the closure, since it may outlive this function call.
    let disp_clone = displayed.to_string();
    let native_clone = native.to_string();
    let disp_label_str = disp_label.to_string();
    let nat_label_str = nat_label.to_string();
    let widget_clone: gtk::Widget = widget.clone().upcast();

    // When the right-click gesture is pressed, build and show the popover menu.
    gesture.connect_pressed(move |_gesture, _n_press, x, y| {
        // Create a new menu model for the popover.
        let menu_model = gio::Menu::new();

        // ---- "Copy Displayed Value" Menu Item ----
        let copy_disp_item = gio::MenuItem::new(Some(&disp_label_str), Some("win.copy-value"));
        let disp_variant = glib::Variant::from(disp_clone.as_str());
        copy_disp_item.set_attribute_value("target", Some(&disp_variant));
        menu_model.append_item(&copy_disp_item);

        // ---- "Copy Native Value" Menu Item ----
        let copy_nat_item = gio::MenuItem::new(Some(&nat_label_str), Some("win.copy-value"));
        let nat_variant = glib::Variant::from(native_clone.as_str());
        copy_nat_item.set_attribute_value("target", Some(&nat_variant));
        menu_model.append_item(&copy_nat_item);

        // ---- Optional "Open Externally" Menu Item ----
        // Only add this item if the native value looks like a URI and there is a handler for it.
        if looks_like_uri(&native_clone) && uri_has_handler(&native_clone).is_ok() {
            let open_item = gio::MenuItem::new(Some("Open Externally"), Some("win.open-uri"));
            let uri_variant = glib::Variant::from(native_clone.as_str());
            open_item.set_attribute_value("target", Some(&uri_variant));
            menu_model.append_item(&open_item);
        }

        // Create a PopoverMenu from the menu model.
        let popover = gtk::PopoverMenu::from_model(Some(&menu_model));

        // ---- Position the Popover Near the Click Location ----

        // Try to compute coordinates relative to the widget's root (main window).
        // If that fails, fall back to using local coordinates.
        let (parent, rect) = if let Some(root) = widget_clone.root() {
            if let Some((rx, ry)) = widget_clone.translate_coordinates(&root, x, y) {
                (
                    root.upcast::<gtk::Widget>(),
                    gdk4::Rectangle::new(rx as i32, ry as i32, 1, 1),
                )
            } else {
                (
                    root.upcast::<gtk::Widget>(),
                    gdk4::Rectangle::new(x as i32, y as i32, 1, 1),
                )
            }
        } else {
            (
                widget_clone.clone(),
                gdk4::Rectangle::new(x as i32, y as i32, 1, 1),
            )
        };

        // Set the parent widget for the popover and anchor it to the click position.
        popover.set_parent(&parent);
        popover.set_pointing_to(Some(&rect));
        // Show the popover menu.
        popover.popup();
    });

    // Attach the gesture controller to the target widget.
    widget.add_controller(gesture);
}

/// Determines whether a given string appears to be a valid URI by attempting to parse it.
///
/// This function uses the `Url` parser to check if the input string is syntactically a URI.
/// It does not guarantee the URI points to a reachable resource—only that it conforms to URI syntax.
///
/// # Arguments
/// * `s` - The string to test for URI validity.
///
/// # Returns
/// * `true` if the string is a syntactically valid URI (according to the `Url` crate), or
/// * `false` otherwise.
fn looks_like_uri(s: &str) -> bool {
    // Attempt to parse the string as a URI using the `Url` crate.
    // If parsing succeeds, return true; otherwise, return false.
    url::Url::parse(s).is_ok()
}

/// Truncates a string to a maximum number of characters, appending an ellipsis if the string was cut off.
///
/// This function iterates over the input string character by character, copying up to
/// `max_chars` Unicode scalar values (not bytes) into a new string. If the string exceeds
/// the allowed length, it appends a Unicode ellipsis character ('…') at the end to indicate
/// that the string was truncated. If the input string is already within the limit, it is returned unchanged.
///
/// # Arguments
/// * `s` - The original string to potentially truncate.
/// * `max_chars` - The maximum number of characters to include before truncation.
///
/// # Returns
/// * A new `String` containing either the original string (if short enough) or a truncated version ending with an ellipsis.
fn ellipsize(s: &str, max_chars: usize) -> String {
    // Initialize a counter for how many characters have been added.
    let mut count = 0;
    // Create a new String to accumulate the output.
    let mut result = String::new();

    // Iterate over each Unicode character (not byte) in the input string.
    for ch in s.chars() {
        // If we've reached the maximum allowed characters,
        // append an ellipsis and stop processing further characters.
        if count >= max_chars {
            result.push('…');
            break;
        }
        // Otherwise, add the character to the result.
        result.push(ch);
        count += 1;
    }

    // If we exited the loop early because the input was too long,
    // return the result string with the ellipsis.
    // Otherwise, if the input was within the limit, return it unchanged.
    if count < s.chars().count() {
        result
    } else {
        s.to_string()
    }
}

/// Converts a URI or predicate name into a more human-friendly label by extracting
/// the last component and inserting spaces between words based on a camel-case interpretation.
///
/// # Arguments
/// * `uri` - The full URI or identifier string to convert.
///
/// # Returns
/// * A `String` containing the label, e.g., "Date Modified" from "http://example.org/DateModified".
fn friendly_label(uri: &str) -> String {
    // Remove any trailing '#' or '/' from the URI, to avoid empty components.
    let trimmed = uri.trim_end_matches(&['#', '/'][..]);

    // Find the last component after a '#' or '/' (the "local name" in RDF).
    // If not found, fall back to the whole trimmed string.
    let last = trimmed.rsplit(&['#', '/'][..]).next().unwrap_or(trimmed);

    // Vector to accumulate each separated word as we split the identifier.
    let mut words = Vec::new();
    // Temporary string to build up each word as we scan.
    let mut cur = String::new();

    // Iterate through each character in the last component.
    for c in last.chars() {
        // If we hit an uppercase letter and we already have content,
        // treat it as the start of a new word and push the current word.
        if c.is_uppercase() && !cur.is_empty() {
            words.push(cur.clone());
            cur.clear();
        }
        // Add the character to the current word-in-progress.
        cur.push(c);
    }
    // After the loop, push any leftover word to the vector.
    if !cur.is_empty() {
        words.push(cur);
    }

    // Now, capitalize the first letter of each word, preserving the rest as is.
    words
        .into_iter()
        .map(|w| {
            let mut cs = w.chars();
            // If the word is non-empty, capitalize the first char and append the rest.
            if let Some(f) = cs.next() {
                f.to_uppercase().collect::<String>() + cs.as_str()
            } else {
                String::new()
            }
        })
        .collect::<Vec<_>>() // Collect all formatted words into a vector.
        .join(" ") // Join the words with spaces for a human-friendly label.
}

/// Formats a native RDF literal value as a user-friendly string for display.
///
/// Currently only translates ISO8601 date-times into "YYYY-MM-DD HH:MM:SS", while passing
/// all other datatypes through as-is.
///
/// # Arguments
/// * `obj` - The raw value as a string.
/// * `dtype` - The datatype URI indicating how the value should be interpreted.
///
/// # Returns
/// * A `String` formatted for display.
fn friendly_value(obj: &str, dtype: &str) -> String {
    // Check if the datatype corresponds to an ISO8601 date-time type.
    if dtype == XSD_DATETYPE {
        // Attempt to parse the value as an ISO8601 date-time using glib::DateTime.
        // If successful, convert to local time and format as "YYYY-MM-DD HH:MM:SS".
        if let Ok(dt) = glib::DateTime::from_iso8601(obj, None)
            .and_then(|dt| dt.to_local())
            .and_then(|ldt| ldt.format("%F %T"))
        {
            // Return the formatted local date-time as a string.
            return dt.to_string();
        }
    }
    // For all other datatypes or if parsing fails, return the original value as-is.
    obj.to_string()
}

/// Fetches the RDF comment (rdfs:comment) for a given predicate URI from the Tracker database, if available.
///
/// This function performs a SPARQL query against the Tracker service to retrieve a human-readable
/// comment or description associated with the specified predicate. It is used to provide
/// contextual tooltips for RDF properties in the user interface.
///
/// # Arguments
/// * `predicate` - The URI of the RDF property whose comment is to be fetched.
///
/// # Returns
/// * `Some(String)` containing the comment if found, or
/// * `None` if the comment is not available or if any error occurs while querying.
fn fetch_comment(predicate: &str) -> Option<String> {
    // Attempt to establish a connection to the Tracker D-Bus SPARQL service.
    // If the connection fails, return None immediately.
    let conn = create_store_connection().ok()?;

    // Prepare a SPARQL query that asks for the comment (rdfs:comment) of the predicate.
    // The query is limited to return at most one comment string (?c).
    let sparql = format!(
        "SELECT ?c WHERE {{ <{pred}> <{comment}> ?c }} LIMIT 1",
        pred = predicate,
        comment = RDFS_COMMENT
    );

    // Execute the query on the Tracker service. If querying fails, return None.
    let cursor = conn.query(&sparql, None::<&gio::Cancellable>).ok()?;

    // If there is a result, extract the comment string from the first column.
    if cursor.next(None::<&gio::Cancellable>).unwrap_or(false) {
        Some(cursor.string(0).unwrap_or_default().to_string())
    } else {
        // If there are no results, return None to indicate that no comment was found.
        None
    }
}

#[cfg(test)]
mod tests {
    // Bring symbols from the parent module into scope so the tests can call
    // helper functions directly.
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

    #[test]
    fn uri_has_handler_unknown_scheme() {
        let uri = "nosuchscheme://foo";
        assert!(uri_has_handler(uri).is_err());
    }
}
