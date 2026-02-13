use ssukka::Obfuscator;
use std::io::{self, Read, Write};
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    match parse_args(&args) {
        Ok(opts) => {
            if opts.help {
                print_usage();
                return;
            }
            if let Err(e) = run(opts) {
                eprintln!("ssukka: {e}");
                process::exit(1);
            }
        }
        Err(msg) => {
            eprintln!("ssukka: {msg}");
            eprintln!("Try 'ssukka --help' for more information.");
            process::exit(1);
        }
    }
}

struct CliOptions {
    input: Option<String>,
    output: Option<String>,
    no_rename: bool,
    no_minify_css: bool,
    no_minify_js: bool,
    no_encode_entities: bool,
    no_shuffle_attrs: bool,
    no_randomize_case: bool,
    seed: Option<u64>,
    help: bool,
}

fn parse_args(args: &[String]) -> std::result::Result<CliOptions, String> {
    let mut opts = CliOptions {
        input: None,
        output: None,
        no_rename: false,
        no_minify_css: false,
        no_minify_js: false,
        no_encode_entities: false,
        no_shuffle_attrs: false,
        no_randomize_case: false,
        seed: None,
        help: false,
    };

    let mut i = 1; // skip program name
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                opts.help = true;
                return Ok(opts);
            }
            "-i" | "--input" => {
                i += 1;
                if i >= args.len() {
                    return Err("missing argument for -i".into());
                }
                opts.input = Some(args[i].clone());
            }
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    return Err("missing argument for -o".into());
                }
                opts.output = Some(args[i].clone());
            }
            "--seed" => {
                i += 1;
                if i >= args.len() {
                    return Err("missing argument for --seed".into());
                }
                opts.seed = Some(
                    args[i]
                        .parse::<u64>()
                        .map_err(|_| format!("invalid seed: {}", args[i]))?,
                );
            }
            "--no-rename" => opts.no_rename = true,
            "--no-minify-css" => opts.no_minify_css = true,
            "--no-minify-js" => opts.no_minify_js = true,
            "--no-encode-entities" => opts.no_encode_entities = true,
            "--no-shuffle-attrs" => opts.no_shuffle_attrs = true,
            "--no-randomize-case" => opts.no_randomize_case = true,
            other => {
                return Err(format!("unknown option: {other}"));
            }
        }
        i += 1;
    }

    Ok(opts)
}

fn run(opts: CliOptions) -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Read input
    let html = match &opts.input {
        Some(path) => std::fs::read_to_string(path)?,
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };

    // Build obfuscator
    let mut builder = Obfuscator::builder();

    if opts.no_rename {
        builder = builder.rename_classes(false).rename_ids(false);
    }
    if opts.no_minify_css {
        builder = builder.minify_css(false);
    }
    if opts.no_minify_js {
        builder = builder.minify_js(false);
    }
    if opts.no_encode_entities {
        builder = builder
            .encode_text_entities(false)
            .encode_attr_entities(false)
            .encode_js_strings(false);
    }
    if opts.no_shuffle_attrs {
        builder = builder.shuffle_attributes(false);
    }
    if opts.no_randomize_case {
        builder = builder.randomize_tag_case(false);
    }
    if let Some(seed) = opts.seed {
        builder = builder.seed(seed);
    }

    let obfuscator = builder.build();
    let result = obfuscator.obfuscate(&html)?;

    // Write output
    match &opts.output {
        Some(path) => std::fs::write(path, &result)?,
        None => {
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            handle.write_all(result.as_bytes())?;
        }
    }

    Ok(())
}

fn print_usage() {
    println!(
        "ssukka — HTML obfuscation tool

USAGE:
    ssukka [OPTIONS]
    ssukka -i input.html -o output.html
    cat input.html | ssukka > output.html

OPTIONS:
    -i, --input <FILE>       Input HTML file (default: stdin)
    -o, --output <FILE>      Output file (default: stdout)
    --seed <N>               Seed for deterministic output
    --no-rename              Disable class/ID renaming
    --no-minify-css          Disable CSS minification
    --no-minify-js           Disable JS minification
    --no-encode-entities     Disable entity encoding
    --no-shuffle-attrs       Disable attribute order shuffling
    --no-randomize-case      Disable tag case randomization
    -h, --help               Print this help message"
    );
}
