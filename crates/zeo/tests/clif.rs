//! CLIF snapshots for a small corpus, the capi-surface presence check
//! against `libzeo.a`, and the deterministic-output golden.

use std::path::PathBuf;

fn clif_of(source: &str) -> String {
    let opts = zeo::CompileOptions::default();
    let text = zeo::compile_to_clif_text(source, &opts).expect("the slice program must lower");
    // The default calling convention names the host (`apple_aarch64`,
    // `system_v`); normalize so the snapshots hold on every platform.
    text.replace("apple_aarch64", "ccall")
        .replace("system_v", "ccall")
}

#[test]
fn clif_snapshot_hello() {
    insta::assert_snapshot!(clif_of("puts \"Hello, world!\"\n"));
}

#[test]
fn clif_snapshot_fib() {
    insta::assert_snapshot!(clif_of(
        "def fib(n)\n  if n < 2\n    n\n  else\n    fib(n - 1) + fib(n - 2)\n  end\nend\nputs fib(10)\n",
    ));
}

/// The inlined accessor under the run-time-redefinition gate: the
/// `zeo_rt_is_live` call, the folded ivar read on one arm and the ordinary
/// implicit send on the other.
///
/// A golden cannot see which arm the site took -- both print the same
/// answer while nothing is live. The snapshot is what pins that the guard
/// is EMITTED, and that it is emitted only here: `read_plain` names an
/// accessor no run-time definition mentions, so it keeps the bare fold.
#[test]
fn clif_snapshot_guarded_accessor() {
    insta::assert_snapshot!(clif_of(
        "class K\n  attr_reader :v, :u\n  def initialize\n    @v = 1\n    @u = 2\n  end\n  def read = v\n  def read_plain = u\nend\nk = K.new\nK.class_eval { define_method(:v) { 99 } }\nputs k.read\nputs k.read_plain\n",
    ));
}

/// A builtin reopen under its positional flag: the `zeo_reopen_flags` load,
/// the forward to the row it replaced on one arm and the reopened body on the
/// other, and the store at the `class Array ... end` marker.
///
/// A golden cannot see which arm ran -- both print the right answer once the
/// class body is above the call. The snapshot is what pins that the guard is
/// emitted at all, and that the forwarded call carries the block.
#[test]
fn clif_snapshot_positional_builtin_reopen() {
    insta::assert_snapshot!(clif_of(
        "p [1, 2].take_while { true }\nclass Array\n  def take_while\n    :array_own\n  end\nend\np [1, 2].take_while { true }\n",
    ));
}

#[test]
fn clif_snapshot_block_send() {
    insta::assert_snapshot!(clif_of(
        "total = 0\nq = Array.new(3, 2)\nq.each { |x| total = total + x }\nputs total\n",
    ));
}

/// A guarded fused `arr.each`: the receiver tag test, `iter_inline_ok_for`,
/// the inline arm and the dynamic fallback arm, all in one function.
///
/// This shape is why slot initialization moved to the entry block. The
/// block parameter's slot is created inside the INLINE arm, and the
/// epilogue releases every slot in `locals` unconditionally from both the
/// normal exit and the shared landing -- so a slot zeroed where it was
/// created is released uninitialised on the fallback path.
///
/// A golden cannot see this. Both arms print the right answer either way,
/// and every attempt to build a small failing program came out clean,
/// because the released bytes are stack garbage that happens to be benign
/// in a short frame. The snapshot is the proof: the zeroing stores belong
/// at the top of the entry block, ahead of the guards.
#[test]
fn clif_snapshot_fused_each_guards() {
    // The receiver must be a local analyze TYPES as an Array, or the block
    // compiles to an ordinary block fn and none of this appears.
    insta::assert_snapshot!(clif_of(
        "def each_of\n  a = [1, 2]\n  a.each { |x| p x }\nend\neach_of\n",
    ));
}

/// An explicit-receiver accessor on a statically-classed local
/// (`a.nxt = b`, `a.nxt`): one call to the guarded attr entry -- fast
/// arm a slot access inside the runtime, slow arm the full explicit
/// send -- instead of the dispatch path.
#[test]
fn clif_snapshot_explicit_accessor() {
    insta::assert_snapshot!(clif_of(
        "class Node\n  attr_accessor :nxt\nend\ndef walk\n  a = Node.new\n  b = Node.new\n  a.nxt = b\n  p a.nxt.nil?\nend\nwalk\n",
    ));
}

/// The nil guards: `x == nil` / `x != nil` branch on the receiver tag
/// under the NilClass-pristine gate (non-nil receivers keep the cached
/// dynamic arm), and `x.nil?` folds to a bare tag test when no scope in
/// the program defines a `nil?` and no blank-slate receiver can exist.
#[test]
fn clif_snapshot_nil_guards() {
    insta::assert_snapshot!(clif_of(
        "def f(x)\n  p x.nil?\n  p(x == nil)\n  p(x != nil)\nend\nf(nil)\nf(1)\n",
    ));
}

/// The typed-Int twin of the guarded `each`: `n.times` on a local analyze
/// types as an Int. The Int tag test, `iter_inline_ok_for` on
/// `Integer#times`, the payload load feeding the counted loop, and the
/// dynamic fallback arm (which is what a Bignum or a reopened `times`
/// takes).
#[test]
fn clif_snapshot_fused_times_guards() {
    insta::assert_snapshot!(clif_of(
        "def times_of\n  n = 3\n  t = 0\n  n.times { |i| t = t + i }\n  p t\nend\ntimes_of\n",
    ));
}

/// `n.upto(m)` on typed-Int locals: the receiver guard, then the LIMIT's
/// own tag test (a Float bound takes the dynamic row -- and that arm
/// re-lowers the argument, which is why only literal/local args fuse),
/// then the counted loop from the receiver's payload through the
/// limit's, inclusive.
#[test]
fn clif_snapshot_fused_upto_guards() {
    insta::assert_snapshot!(clif_of(
        "def upto_of\n  n = 2\n  m = 5\n  t = 0\n  n.upto(m) { |i| t = t + i }\n  p t\nend\nupto_of\n",
    ));
}

/// The accumulator seam's heap kind: `arr.map` under the same guards as
/// the fused `each`, with the Array accumulator in an epilogue-registered
/// slot (its zeroing store sits in the entry block with the locals'),
/// `zeo_rt_array_push` consuming each iteration value in the latch, and
/// the bit-move + nil at the normal exit.
#[test]
fn clif_snapshot_fused_map_acc() {
    insta::assert_snapshot!(clif_of(
        "def map_of\n  a = [1, 2]\n  p a.map { |x| x * 2 }\nend\nmap_of\n",
    ));
}

/// `arr.sum { .. }`: the Int-0 seed, the raw compensation/generic state
/// slot, `zeo_rt_sum_step` consuming each value in the latch (fallible --
/// a generic `+` can raise), and `zeo_rt_sum_finish` at the normal exit.
#[test]
fn clif_snapshot_fused_sum_acc() {
    insta::assert_snapshot!(clif_of(
        "def sum_of\n  a = [1, 2]\n  p a.sum { |x| x * 2 }\nend\nsum_of\n",
    ));
}

/// `arr.inject(init) { |acc, e| .. }`: the seed lowered in the fast arm
/// only (the slow arm lowers its own copy as the send argument), the
/// accumulator borrowed into the first param's shadow each iteration, and
/// the block value replacing it in the latch.
#[test]
fn clif_snapshot_fused_inject_acc() {
    insta::assert_snapshot!(clif_of(
        "def inject_of\n  a = [1, 2]\n  p a.inject(0) { |acc, e| acc + e }\nend\ninject_of\n",
    ));
}

/// `arr.each_with_index { |e, i| .. }`: the two-name bind -- the element
/// fetch, then the plain counter as an Int.
#[test]
fn clif_snapshot_fused_each_with_index() {
    insta::assert_snapshot!(clif_of(
        "def ewi_of\n  a = [1, 2]\n  a.each_with_index { |e, i| p [e, i] }\nend\newi_of\n",
    ));
}

/// A send whose argument list runs before its literal block: the argument
/// is lowered, and only then does `zeo_rt_proc_new` build the proc.
///
/// The order is load-bearing, not cosmetic. The proc is MOVED to the
/// callee, so no landing can release it; an argument that raises between
/// the two leaks it, which is what
/// `a_raise_in_an_argument_keeps_the_block_unbuilt.rb` measures. It is
/// also ruby's own order -- the receiver and the arguments run first, and
/// the block is made last.
#[test]
fn clif_snapshot_block_after_arguments() {
    insta::assert_snapshot!(clif_of(
        "def take(n)\n  yield n\nend\ndef arg\n  1\nend\ndef go\n  take(arg) { |x| p x }\nend\ngo\n",
    ));
}

/// The floored-division and spaceship integer shapes: the sign-fix on
/// `/` and `%`, the zero-divisor guard, and `<=>`'s three-way select.
#[test]
fn clif_snapshot_int_div_mod_spaceship() {
    insta::assert_snapshot!(clif_of(
        "def m(a, b)\n  p a / b\n  p a % b\n  p a <=> b\nend\nm(7, 2)\n",
    ));
}

/// The bit-op integer shapes (`<<` with its overflow escape, `>>`, `&`,
/// `|`, `^`) and the multiply-overflow arm.
#[test]
fn clif_snapshot_int_bits_and_mul() {
    insta::assert_snapshot!(clif_of(
        "def m(a, b)\n  p a << b\n  p a >> b\n  p a & b\n  p a | b\n  p a ^ b\n  p a * b\nend\nm(5, 2)\n",
    ));
}

/// The float arms: plain arithmetic, the fallible `%`, and the float
/// spaceship. `ARGV.length` keeps the operands out of the constant folder.
#[test]
fn clif_snapshot_float_binops() {
    insta::assert_snapshot!(clif_of(
        "x = ARGV.length + 1.5\ny = ARGV.length + 2.5\np x + y\np x * y\np x % y\np x <=> y\n",
    ));
}

/// The `defined?` value forms: an ivar, a predefined global, a bare
/// method probe, and a constant path.
#[test]
fn clif_snapshot_defined_forms() {
    insta::assert_snapshot!(clif_of(
        "p defined?(@x)\np defined?($~)\np defined?(puts)\np defined?(Math::PI)\n",
    ));
}

/// `defined?(yield)` and `defined?(super)` -- the two forms that read the
/// frame rather than a name.
#[test]
fn clif_snapshot_defined_yield_super() {
    insta::assert_snapshot!(clif_of(
        "class A\n  def go = p [defined?(yield), defined?(super)]\nend\nclass B < A\n  def go = super\nend\nB.new.go\n",
    ));
}

/// A read of a `private_constant` name through `M::X`: the guard call
/// ahead of the scoped read.
#[test]
fn clif_snapshot_private_constant_guard() {
    insta::assert_snapshot!(clif_of(
        "module M\n  X = 1\n  private_constant :X\nend\nbegin\n  p M::X\nrescue NameError => e\n  puts e.message\nend\n",
    ));
}

/// A bare constant read from inside a nested module body: the cref walk,
/// not a receiver-scoped read.
#[test]
fn clif_snapshot_const_cref_walk() {
    insta::assert_snapshot!(clif_of(
        "module A\n  module B\n    X = 1\n    def self.go = p X\n  end\nend\nA::B.go\n",
    ));
}

/// `"lit".freeze` on a pristine `freeze`: folds to the pooled frozen
/// string, no send.
#[test]
fn clif_snapshot_frozen_literal_fold() {
    insta::assert_snapshot!(clif_of("p \"lit\".freeze\n"));
}

/// A safe-navigation send: the nil-test diamond around the send.
#[test]
fn clif_snapshot_safe_navigation() {
    insta::assert_snapshot!(clif_of("x = ARGV[0]\np x&.length\n"));
}

/// A block-argument send (`&f`): the proc coercion path, not a literal
/// block.
#[test]
fn clif_snapshot_block_arg_send() {
    insta::assert_snapshot!(clif_of(
        "def go\n  f = proc { |x| p x }\n  [1].each(&f)\nend\ngo\n",
    ));
}

/// A keyword send onto a keyword-defaulted def: the kwargs packing and
/// the callee's default fill.
#[test]
fn clif_snapshot_keyword_send() {
    insta::assert_snapshot!(clif_of(
        "def kw(a, k: 1)\n  p [a, k]\nend\nkw(2, k: 3)\nkw(4)\n",
    ));
}

/// Two compiles of one program are byte-identical -- object emission is a
/// pure function of the CLIF.
#[test]
fn object_output_is_deterministic() {
    let src = "def add(a, b)\n  a + b\nend\nputs add(2, 3)\n";
    let a = zeo::compile_to_object_with(src, &zeo::CompileOptions::default(), false)
        .expect("compiles")
        .object;
    let b = zeo::compile_to_object_with(src, &zeo::CompileOptions::default(), false)
        .expect("compiles")
        .object;
    assert_eq!(a, b, "two compiles must produce identical object bytes");
}

/// Every symbol the emitter can import (`clif::capi_names::CAPI`) is a
/// defined `T` symbol in the runtime archive the link consumes.
#[test]
fn capi_surface_is_exported_by_the_archive() {
    let mut dir = std::env::current_exe().expect("test binary path");
    // target/<profile>/deps/<bin> -> target/<profile>
    dir.pop();
    dir.pop();
    let archive: PathBuf = dir.join("libzeo.a");
    assert!(
        archive.is_file(),
        "libzeo.a must sit beside the test profile dir: {}",
        archive.display()
    );
    let out = std::process::Command::new("nm")
        .arg(&archive)
        .output()
        .expect("nm must run");
    // macOS nm exits nonzero for archive members it cannot fully parse
    // (bitcode attribute drift, symbol-less members); the symbol listing
    // on stdout is still complete for the runtime's own objects, which is
    // all this asserts over.
    let nm = String::from_utf8_lossy(&out.stdout);
    assert!(
        !nm.is_empty(),
        "nm produced no listing for {}",
        archive.display()
    );
    let defined: std::collections::HashSet<&str> = nm
        .lines()
        .filter(|l| l.contains(" T "))
        .filter_map(|l| l.rsplit(' ').next())
        .map(|s| s.strip_prefix('_').unwrap_or(s))
        .collect();
    for row in zeo::clif::capi_names::CAPI {
        assert!(
            defined.contains(row.name),
            "capi_names row `{}` is not a defined symbol in libzeo.a",
            row.name
        );
    }
    // Data symbols live in data/bss sections, never text -- the " T "
    // filter above would pass vacuously for them.
    let defined_data: std::collections::HashSet<&str> = nm
        .lines()
        .filter(|l| l.contains(" D ") || l.contains(" S ") || l.contains(" B "))
        .filter_map(|l| l.rsplit(' ').next())
        .map(|s| s.strip_prefix('_').unwrap_or(s))
        .collect();
    for name in zeo::clif::capi_names::CAPI_DATA {
        assert!(
            defined_data.contains(name),
            "capi_names data row `{name}` is not a defined data symbol in libzeo.a"
        );
    }
}

/// `CLASS_TABLE_SYMBOLS` names EVERY `zeo_ctable_*` the archive defines, and
/// nothing it does not.
///
/// Both directions are failures with no other detector. A symbol the list
/// misses is a builtin class whose table no program ever names -- it loses
/// every method and every constant, silently, and only at run time. A name in
/// the list that the archive lacks is a link error in every program.
///
/// The list is scanned out of the runtime's sources by build.rs rather than
/// derived from `CLASS_SURFACE`, which reads only `builtins/` and `ext/`: at
/// the time of writing that was five classes short (`Ractor` lives at
/// `src/ractor.rs`, `FFI::Type` outside `ext/`), and two heuristics that
/// looked right produced phantom names from `let x = zeo_abi::..` lines.
#[test]
fn class_tables_are_complete() {
    let mut dir = std::env::current_exe().expect("test binary path");
    dir.pop();
    dir.pop();
    let archive: PathBuf = dir.join("libzeo.a");
    assert!(
        archive.is_file(),
        "libzeo.a must sit beside the test profile dir"
    );
    let out = std::process::Command::new("nm")
        .arg(&archive)
        .output()
        .expect("nm must run");
    let nm = String::from_utf8_lossy(&out.stdout);
    assert!(!nm.is_empty(), "nm produced no listing");
    let in_archive: std::collections::BTreeSet<String> = nm
        .lines()
        .filter_map(|l| l.rsplit(' ').next())
        .map(|s| s.strip_prefix('_').unwrap_or(s))
        .filter(|s| s.starts_with("zeo_ctable_"))
        .map(str::to_string)
        .collect();
    let listed: std::collections::BTreeSet<String> = zeo::builtin_surface::CLASS_TABLE_SYMBOLS
        .iter()
        .map(|(_, s)| (*s).to_string())
        .collect();
    let missing: Vec<&String> = in_archive.difference(&listed).collect();
    let phantom: Vec<&String> = listed.difference(&in_archive).collect();
    assert!(
        missing.is_empty() && phantom.is_empty(),
        "CLASS_TABLE_SYMBOLS is out of step with libzeo.a\n           in the archive but NOT listed (these classes would lose every \
         method and constant): {missing:?}\n           listed but NOT in the archive (a link error in every program): {phantom:?}"
    );
    assert!(!listed.is_empty(), "no class tables at all");
}

/// Every symbol the emitter can import resolves through the runtime's
/// in-process table (`zeo_rt::capi::symbols`) -- what the JIT run path
/// links against (the `zeo` binary does not export `zeo_rt_*`, so this
/// table IS the JIT's symbol source; a missing row would fail a user's
/// program at JIT relocation).
#[test]
fn capi_surface_resolves_in_process() {
    for row in zeo::clif::capi_names::CAPI {
        assert!(
            zeo::zeo_rt::capi::symbols::addr(row.name).is_some(),
            "capi_names row `{}` has no in-process address (add it to zeo-rt capi/symbols.rs)",
            row.name
        );
    }
    for name in zeo::clif::capi_names::CAPI_DATA {
        assert!(
            zeo::zeo_rt::capi::symbols::data_addr(name).is_some(),
            "capi_names data row `{name}` has no in-process address (add it to zeo-rt capi/symbols.rs)"
        );
    }
}

/// The capi table itself is pinned -- an accidental signature change on
/// the emitter side shows up as a reviewed snapshot diff.
#[test]
fn capi_table_snapshot() {
    let mut rendered = String::new();
    for row in zeo::clif::capi_names::CAPI {
        rendered.push_str(&format!(
            "{} ({:?}) -> {:?}\n",
            row.name, row.params, row.ret
        ));
    }
    insta::assert_snapshot!(rendered);
}
