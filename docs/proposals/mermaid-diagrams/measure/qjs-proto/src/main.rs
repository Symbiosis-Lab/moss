use rquickjs::{Context, Runtime, Function, Object};
use std::time::Instant;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bundle = std::fs::read_to_string(&args[1]).unwrap();
    let dir = &args[2];
    let out_dir = args.get(3).cloned();
    let t0 = Instant::now();
    let rt = Runtime::new().unwrap();
    rt.set_memory_limit(256 << 20);
    rt.set_max_stack_size(8 << 20);
    let deadline = std::rc::Rc::new(std::cell::Cell::new(None::<Instant>));
    { let d = deadline.clone(); rt.set_interrupt_handler(Some(Box::new(move || d.get().map_or(false, |t| Instant::now() > t)))); }
    let budget_ms: u64 = std::env::var("BUDGET_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(2000);
    let ctx = Context::full(&rt).unwrap();
    ctx.with(|ctx| {
        ctx.eval::<(), _>(bundle.as_bytes()).unwrap();
        // glue: wrap render with fixed options, return svg or error string
        ctx.eval::<(), _>(r#"
          globalThis.__render = function(src, prefix) {
            try { return "OK" + pele.render(src, {idPrefix: prefix, now: new Date('2026-01-01T00:00:00Z')}).svg; }
            catch (e) { return "ER" + (e && e.code || '') + ":" + (e && e.message); }
          };"#).unwrap();
    });
    let t_load = t0.elapsed();
    println!("mem after load: {} KB", rt.memory_usage().memory_used_size / 1024);
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().map_or(false, |x| x == "mmd")).collect();
    entries.sort();
    let mut total = std::time::Duration::ZERO;
    for p in &entries {
        let src = std::fs::read_to_string(p).unwrap();
        let s = Instant::now();
        deadline.set(Some(s + std::time::Duration::from_millis(budget_ms)));
        let r: String = ctx.with(|ctx| {
            let f: Function = ctx.globals().get("__render").unwrap();
            match f.call::<_, String>((src.as_str(), "d0")) { Ok(v) => v, Err(e) => format!("XX{}", e) }
        });
        deadline.set(None);
        let el = s.elapsed(); total += el;
        let name = p.file_name().unwrap().to_string_lossy();
        println!("{}\t{}\t{:.1}ms\t{}B\tmem={}KB\t{}", name, &r[..2], el.as_secs_f64()*1e3, r.len()-2, rt.memory_usage().memory_used_size/1024, if &r[..2]=="OK" {String::new()} else {r.chars().take(90).collect()});
        if let Some(od) = &out_dir { std::fs::write(format!("{}/{}.svg", od, name), &r[2..]).unwrap(); }
    }
    let _ = Object::new; 
    println!("load={:.0}ms renders={:.0}ms n={}", t_load.as_secs_f64()*1e3, total.as_secs_f64()*1e3, entries.len());
}
