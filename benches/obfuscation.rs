//! Throughput benchmarks for the obfuscation presets. Run with `cargo bench`.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ssukka::config::JsStringEncoding;
use ssukka::Obfuscator;
use std::hint::black_box;

const PAGE: &str = r##"<!doctype html><html><head>
<style>.product-card{border:1px solid #ccc}.price-tag{color:green}#checkout{display:block}</style>
</head><body>
<div class="product-card" id="checkout">
  <h2 class="product-title">Wireless Headphones</h2>
  <span class="price-tag">$129.99</span>
  <p class="description">Premium noise cancelling headphones with long battery life.</p>
  <a href="#checkout" class="buy-button">Buy now</a>
</div>
<script>
  const cart = [];
  function addToCart(productId) {
    document.getElementById("checkout").classList.add("active");
    cart.push(productId);
    console.log("Added product " + productId + " to cart");
  }
  document.querySelector(".buy-button").addEventListener("click", function () {
    addToCart("headphones-001");
  });
</script>
</body></html>"##;

fn presets() -> Vec<(&'static str, Obfuscator)> {
    vec![
        ("cosmetic", Obfuscator::builder().seed(1).build()),
        (
            "honeypots",
            Obfuscator::builder()
                .seed(1)
                .inject_honeypots(true)
                .honeypot_count(8)
                .build(),
        ),
        (
            "structural",
            Obfuscator::builder().seed(1).structural_obfuscation(true).build(),
        ),
        (
            "ast-full",
            Obfuscator::builder()
                .seed(1)
                .js_ast(true)
                .mangle_identifiers(true)
                .js_string_encoding(JsStringEncoding::Array)
                .dead_code_injection(true)
                .control_flow_flattening(true)
                .build(),
        ),
    ]
}

fn bench_presets(c: &mut Criterion) {
    let mut group = c.benchmark_group("presets");
    group.throughput(Throughput::Bytes(PAGE.len() as u64));
    for (name, obf) in presets() {
        group.bench_function(name, |b| b.iter(|| obf.obfuscate(black_box(PAGE)).unwrap()));
    }
    group.finish();
}

fn bench_scaling(c: &mut Criterion) {
    let obf = Obfuscator::builder().seed(1).build();
    let mut group = c.benchmark_group("scaling-cosmetic");
    for reps in [1usize, 10, 50] {
        let input = PAGE.repeat(reps);
        group.throughput(Throughput::Bytes(input.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(reps), &input, |b, inp| {
            b.iter(|| obf.obfuscate(black_box(inp)).unwrap())
        });
    }
    group.finish();
}

criterion_group!(benches, bench_presets, bench_scaling);
criterion_main!(benches);
