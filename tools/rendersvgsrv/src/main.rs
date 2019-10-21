// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::fmt;
use std::fs;
use std::io::Write;
use std::path;
use std::str;
use resvg::prelude::*;
use std::ffi::OsString;
use std::path::PathBuf;
use std::fs::File;
extern crate image;

extern crate simple_server;
use simple_server::Server;
use simple_server::StatusCode;
use simple_server::Method;

mod args;


macro_rules! bail {
    ($msg:expr) => {
        return Err(format!("{}", $msg));
    };
    ($fmt:expr, $($arg:tt)*) => {
        return Err(format!($fmt, $($arg)*));
    };
}

fn main() {
    let host = "0.0.0.0";
    let port = "7878";

    let server = Server::new(|request, mut response| {
        if request.method() == Method::POST && request.uri() == "/convert" {
            let params = str::from_utf8(request.body());

            let args: Vec<OsString> = params.unwrap().split('|').map(|x| OsString::from(x)).collect();
            eprintln!("Request received. {} {} {:?}", request.method(), request.uri(), args);

            if let Err(e) = process(args) {
                response.status(StatusCode::NOT_FOUND);
                Ok(response.body(format!("error: {}", e).as_bytes().to_vec())?)
            } else {
                Ok(response.body("ok\n".as_bytes().to_vec())?)
            }
            
        } else {
            response.status(StatusCode::NOT_FOUND);
            Ok(response.body("404 not found!".as_bytes().to_vec())?)
        }
        
    });

    server.listen(host, port);
}

fn main_old() {
    let mut osargs: Vec<_> = std::env::args_os().collect();
    osargs.remove(0);

    if let Err(e) = process(osargs) {
        eprintln!("Error: {}.", e);
        std::process::exit(1);
    }
}

fn process(osargs: Vec<OsString>) -> Result<(), String> {
    #[cfg(all(not(feature = "cairo-backend"),
              not(feature = "qt-backend"),
              not(feature = "skia-backend"),
              not(feature = "raqote-backend")))]
    {
        bail!("rendersvg has been built without any backends")
    }

    let (args, opt) = match args::parse(osargs) {
        Ok((args, opt)) => (args, opt),
        Err(e) => {
            args::print_help();
            println!();
            bail!(e)
        }
    };

    // Do not print warning during the ID querying.
    //
    // Some crates still can print to stdout/stderr, but we can't do anything about it.
    /*if !(args.query_all || args.quiet) {
        fern::Dispatch::new()
            .format(log_format)
            .level(log::LevelFilter::Warn)
            .chain(std::io::stderr())
            .apply()
            .unwrap();
    }*/

    let backend: Box<dyn Render> = match args.backend_name.as_str() {
        #[cfg(feature = "cairo-backend")]
        "cairo" => Box::new(resvg::backend_cairo::Backend),
        #[cfg(feature = "qt-backend")]
        "qt" => Box::new(resvg::backend_qt::Backend),
        #[cfg(feature = "skia-backend")]
        "skia" => Box::new(resvg::backend_skia::Backend),
        #[cfg(feature = "raqote-backend")]
        "raqote" => Box::new(resvg::backend_raqote::Backend),
        _ => bail!("unknown backend"),
    };

    macro_rules! timed {
        ($name:expr, $task:expr) => { run_task(args.perf, $name, || $task) };
    }

    // Load file.
    let tree = timed!("Preprocessing", {
        usvg::Tree::from_file(&args.in_svg, &opt.usvg).map_err(|e| e.to_string())
    })?;

    if args.query_all {
        return query_all(&tree);
    }

    // Dump before rendering in case of panic.
    if let Some(ref dump_path) = args.dump {
        dump_svg(&tree, dump_path)?;
    }

    if args.pretend {
        return Ok(());
    }

    // Render.
    if let Some(ref out_file) = args.out_png {
        let img = if let Some(ref id) = args.export_id {
            if let Some(node) = tree.root().descendants().find(|n| &*n.id() == id) {
                timed!("Rendering", backend.render_node_to_image(&node, &opt))
            } else {
                bail!("SVG doesn't have '{}' ID", id)
            }
        } else {
            timed!("Rendering", backend.render_to_image(&tree, &opt))
        };

        match img {
            Some(mut img) => { 
                if let Some(jpeg_level) = args.jpeg {
                    let tmpfile = PathBuf::from(format!("{}.tmp.png", out_file.to_str().unwrap()));
                    img.save_png(&tmpfile);
                    let img = image::open(&tmpfile).expect("Opening image failed");

                    let rgbimg = img.to_rgb();

                    let mut out = File::create(out_file).unwrap();
                    {
                        let level = jpeg_level as u8;
                        let mut j = image::jpeg::JPEGEncoder::new_with_quality(&mut out, level);
                        j.encode(&rgbimg, rgbimg.width(), rgbimg.height(), image::ColorType::RGB(8)).expect("!encode");
                    }
                    fs::remove_file(tmpfile);


                } else {
                    timed!("Saving", img.save_png(out_file));
                }
            }
            None => { bail!("failed to allocate an image") }
        }
    };

    Ok(())
}

fn query_all(tree: &usvg::Tree) -> Result<(), String> {
    let mut count = 0;
    for node in tree.root().descendants() {
        if tree.is_in_defs(&node) {
            continue;
        }

        if node.id().is_empty() {
            continue;
        }

        count += 1;

        fn round_len(v: f64) -> f64 {
            (v * 1000.0).round() / 1000.0
        }

        if let Some(bbox) = node.calculate_bbox() {
            println!(
                "{},{},{},{},{}", node.id(),
                round_len(bbox.x()), round_len(bbox.y()),
                round_len(bbox.width()), round_len(bbox.height())
            );
        }
    }

    if count == 0 {
        bail!("the file has no valid ID's");
    }

    Ok(())
}

fn run_task<P, T>(perf: bool, title: &str, p: P) -> T
    where P: FnOnce() -> T
{
    if perf {
        let start = time::precise_time_ns();
        let res = p();
        let end = time::precise_time_ns();
        println!("{}: {:.2}ms", title, (end - start) as f64 / 1_000_000.0);
        res
    } else {
        p()
    }
}

fn dump_svg(tree: &usvg::Tree, path: &path::Path) -> Result<(), String> {
    let mut f = fs::File::create(path)
        .map_err(|_| format!("failed to create a file {:?}", path))?;

    f.write_all(tree.to_string(usvg::XmlOptions::default()).as_bytes())
     .map_err(|_| format!("failed to write a file {:?}", path))?;

    Ok(())
}

fn log_format(
    out: fern::FormatCallback,
    message: &fmt::Arguments,
    record: &log::Record,
) {
    let lvl = match record.level() {
        log::Level::Error => "Error",
        log::Level::Warn  => "Warning",
        log::Level::Info  => "Info",
        log::Level::Debug => "Debug",
        log::Level::Trace => "Trace",
    };

    out.finish(format_args!(
        "{} (in {}:{}): {}",
        lvl,
        record.target(),
        record.line().unwrap_or(0),
        message
    ))
}
