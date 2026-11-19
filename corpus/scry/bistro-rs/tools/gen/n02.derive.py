# n02 (N, adapters-main), hand-written on the base: main.rs chooses the in-memory adapter by default and the file adapter with --file (main still chooses)
sub("main.rs", "//! Run one request: `bistro [--memory] <route> key=value…`, e.g.", "//! Run one request: `bistro [--file] <route> key=value…` (bookings kept in memory unless --file), e.g.")
sub("main.rs", """    let memory = args.first().is_some_and(|a| a == "--memory");
    if memory {""", """    let file = args.first().is_some_and(|a| a == "--file");
    if file {""")
sub("main.rs", 'eprintln!("usage: bistro [--memory] <route> key=value…");', 'eprintln!("usage: bistro [--file] <route> key=value…");')
sub("main.rs", """    let app = create(if memory {
        None
    } else {
        Some(Path::new("bistro.db"))
    });""", """    let app = create(if file {
        Some(Path::new("bistro.db"))
    } else {
        None
    });""")
