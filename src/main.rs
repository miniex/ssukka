use ssukka::config::JsStringEncoding;
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
        },
        Err(msg) => {
            eprintln!("ssukka: {msg}");
            eprintln!("Try 'ssukka --help' for more information.");
            process::exit(1);
        },
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
    comment_split: bool,
    seed: Option<u64>,
    honeypots: Option<usize>,
    structural: bool,
    polymorphic: bool,
    js_encoding: Option<JsStringEncoding>,
    js_ast: bool,
    mangle: bool,
    poison_names: bool,
    cff: bool,
    dead_code: bool,
    dead_code_threshold: Option<f32>,
    self_defending: bool,
    watermark: Option<u64>,
    ai_opt_out: bool,
    inline_local: bool,
    base_dir: Option<String>,
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
        comment_split: false,
        seed: None,
        honeypots: None,
        structural: false,
        polymorphic: false,
        js_encoding: None,
        js_ast: false,
        mangle: false,
        poison_names: false,
        cff: false,
        dead_code: false,
        dead_code_threshold: None,
        self_defending: false,
        watermark: None,
        ai_opt_out: false,
        inline_local: false,
        base_dir: None,
        help: false,
    };

    let mut i = 1; // skip program name
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                opts.help = true;
                return Ok(opts);
            },
            "-i" | "--input" => {
                i += 1;
                if i >= args.len() {
                    return Err("missing argument for -i".into());
                }
                opts.input = Some(args[i].clone());
            },
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    return Err("missing argument for -o".into());
                }
                opts.output = Some(args[i].clone());
            },
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
            },
            "--no-rename" => opts.no_rename = true,
            "--no-minify-css" => opts.no_minify_css = true,
            "--no-minify-js" => opts.no_minify_js = true,
            "--no-encode-entities" => opts.no_encode_entities = true,
            "--no-shuffle-attrs" => opts.no_shuffle_attrs = true,
            "--no-randomize-case" => opts.no_randomize_case = true,
            "--comment-split" => opts.comment_split = true,
            "--honeypots" => {
                i += 1;
                if i >= args.len() {
                    return Err("missing argument for --honeypots".into());
                }
                let n = args[i]
                    .parse::<usize>()
                    .map_err(|_| format!("invalid honeypot count: {}", args[i]))?;
                opts.honeypots = Some(n);
            },
            "--structural" => opts.structural = true,
            "--polymorphic" => opts.polymorphic = true,
            "--js-string-encoding" => {
                i += 1;
                if i >= args.len() {
                    return Err("missing argument for --js-string-encoding".into());
                }
                opts.js_encoding = Some(match args[i].as_str() {
                    "none" => JsStringEncoding::None,
                    "escapes" => JsStringEncoding::Escapes,
                    "array" => JsStringEncoding::Array,
                    other => return Err(format!("invalid encoding: {other} (none|escapes|array)")),
                });
            },
            "--js-ast" => opts.js_ast = true,
            "--mangle" => opts.mangle = true,
            "--poison-names" => opts.poison_names = true,
            "--cff" => opts.cff = true,
            "--dead-code" => opts.dead_code = true,
            "--self-defending" => opts.self_defending = true,
            "--dead-code-threshold" => {
                i += 1;
                if i >= args.len() {
                    return Err("missing argument for --dead-code-threshold".into());
                }
                opts.dead_code_threshold = Some(
                    args[i]
                        .parse::<f32>()
                        .map_err(|_| format!("invalid threshold: {}", args[i]))?,
                );
            },
            "--watermark" => {
                i += 1;
                if i >= args.len() {
                    return Err("missing argument for --watermark".into());
                }
                opts.watermark = Some(
                    args[i]
                        .parse::<u64>()
                        .map_err(|_| format!("invalid watermark id: {}", args[i]))?,
                );
            },
            "--ai-opt-out" => opts.ai_opt_out = true,
            "--inline-local-resources" => opts.inline_local = true,
            "--base-dir" => {
                i += 1;
                if i >= args.len() {
                    return Err("missing argument for --base-dir".into());
                }
                opts.base_dir = Some(args[i].clone());
            },
            other => {
                return Err(format!("unknown option: {other}"));
            },
        }
        i += 1;
    }

    Ok(opts)
}

fn run(opts: CliOptions) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let html = match &opts.input {
        Some(path) => std::fs::read_to_string(path)?,
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            buf
        },
    };

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
    if opts.comment_split {
        builder = builder.split_words(true);
    }
    if let Some(n) = opts.honeypots {
        builder = builder.inject_honeypots(true).honeypot_count(n);
    }
    if opts.structural {
        builder = builder.structural_obfuscation(true);
    }
    if opts.polymorphic {
        builder = builder.polymorphic(true);
    }
    if let Some(enc) = opts.js_encoding {
        builder = builder.js_string_encoding(enc);
    }
    if opts.js_ast {
        builder = builder.js_ast(true);
    }
    if opts.mangle {
        builder = builder.js_ast(true).mangle_identifiers(true);
    }
    if opts.poison_names {
        builder = builder.js_ast(true).poison_names(true);
    }
    if opts.cff {
        builder = builder.js_ast(true).control_flow_flattening(true);
    }
    if opts.dead_code {
        builder = builder.js_ast(true).dead_code_injection(true);
    }
    if opts.self_defending {
        builder = builder.js_ast(true).self_defending(true);
    }
    if let Some(t) = opts.dead_code_threshold {
        builder = builder.dead_code_threshold(t);
    }
    if let Some(id) = opts.watermark {
        builder = builder.watermark(id);
    }
    if opts.ai_opt_out {
        builder = builder.emit_ai_opt_out(true);
    }
    if opts.inline_local {
        builder = builder.inline_local_resources(true);
    }
    if let Some(dir) = &opts.base_dir {
        builder = builder.base_dir(dir.clone());
    } else if opts.inline_local {
        // Default the resource root to the input file's directory.
        if let Some(parent) = opts.input.as_deref().and_then(|p| std::path::Path::new(p).parent()) {
            if !parent.as_os_str().is_empty() {
                builder = builder.base_dir(parent);
            }
        }
    }
    if let Some(seed) = opts.seed {
        builder = builder.seed(seed);
    }

    warn_aggressive(&opts);

    let obfuscator = builder.build();
    let result = obfuscator.obfuscate(&html)?;

    match &opts.output {
        Some(path) => std::fs::write(path, &result)?,
        None => {
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            handle.write_all(result.as_bytes())?;
        },
    }

    Ok(())
}

/// Print stderr warnings for options that change the DOM, accessibility, or SEO,
/// naming the affected consumer so the cost is never silent.
fn warn_aggressive(opts: &CliOptions) {
    let mut warnings: Vec<&str> = Vec::new();
    if opts.structural {
        warnings.push("--structural hides text behind JS: no-JS clients and most AI crawlers see empty nodes, and SEO/accessibility degrade until the restore script runs");
    }
    if opts.watermark.is_some() {
        warnings.push("--watermark embeds invisible zero-width characters, which can affect screen readers and programmatic text matching");
    }
    if opts.honeypots.is_some() {
        warnings.push("--honeypots injects hidden decoy nodes into the DOM");
    }
    for w in &warnings {
        eprintln!("ssukka: warning: {w}");
    }
}

fn print_usage() {
    println!(
        "ssukka - HTML obfuscation tool

USAGE:
    ssukka [OPTIONS]
    ssukka -i input.html -o output.html
    cat input.html | ssukka > output.html

OPTIONS:
    -i, --input <FILE>       Input HTML file (default: stdin)
    -o, --output <FILE>      Output file (default: stdout)
    --seed <N>               Seed for deterministic output

  Cosmetic (on by default):
    --no-rename              Disable class/ID renaming
    --no-minify-css          Disable CSS minification
    --no-minify-js           Disable JS minification
    --no-encode-entities     Disable entity encoding
    --no-shuffle-attrs       Disable attribute order shuffling
    --no-randomize-case      Disable tag case randomization
    --js-string-encoding <none|escapes|array>
                             JS string-literal strategy (default: escapes)

  Advanced (opt-in - change DOM/size/runtime; see README threat model):
    --comment-split          Split long words with empty comments (anti-regex-scraper)
    --honeypots <N>          Inject N invisible decoy nodes (scraper traps)
    --structural             Move text into encoded attrs, restore client-side
    --polymorphic            Randomize transforms per run (no fixed seed)
    --js-ast                 Use the oxc AST engine for <script> JS
    --mangle                 Scope-aware local identifier renaming (implies --js-ast)
    --poison-names           Rename locals to misleading names (implies --js-ast)
    --cff                    Control-flow flattening (implies --js-ast)
    --dead-code              Opaque-predicate dead code injection (implies --js-ast)
    --self-defending         Disable console if the script is beautified (implies --js-ast)
    --dead-code-threshold <0..1>   Fraction of sites that get dead code
    --watermark <N>          Embed an invisible zero-width id for provenance
    --ai-opt-out             Inject <meta> AI opt-out signals into <head>
    --inline-local-resources Inline local <link>/<script src> (offline only)
    --base-dir <DIR>         Base directory for resolving local resources

    -h, --help               Print this help message"
    );
}
