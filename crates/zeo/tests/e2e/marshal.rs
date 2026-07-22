//! `Marshal` wire-format + round-trip conformance. Every expected byte string
//! and round-trip value is pinned to the ruby 4.0.5 oracle
//! (`--disable-error_highlight --disable-did_you_mean`).

use crate::support::run_ruby;

#[test]
fn string_encoding_ivar_wire_and_roundtrip() {
    // A String's `I`-wrapper carries its encoding: `E => true` (UTF-8),
    // `E => false` (US-ASCII), `encoding => "<name>"` (other), and NO wrapper
    // for ASCII-8BIT. The encoding survives a round-trip.
    let result = run_ruby(
        r#"
        def wire(x) = Marshal.dump(x).bytes.join(",")
        def rt(x) = Marshal.load(Marshal.dump(x))
        puts wire("abc")
        puts wire("abc".b)
        puts wire("abc".encode("US-ASCII"))
        puts wire("café")
        puts wire("abc".encode("Shift_JIS"))
        puts rt("café").encoding.name
        puts rt("abc".b).encoding.name
        puts rt("abc".encode("Shift_JIS")).encoding.name
        puts rt("hello")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "4,8,73,34,8,97,98,99,6,58,6,69,84\n\
         4,8,34,8,97,98,99\n\
         4,8,73,34,8,97,98,99,6,58,6,69,70\n\
         4,8,73,34,10,99,97,102,195,169,6,58,6,69,84\n\
         4,8,73,34,8,97,98,99,6,58,13,101,110,99,111,100,105,110,103,34,14,83,104,105,102,116,95,74,73,83\n\
         UTF-8\n\
         ASCII-8BIT\n\
         Shift_JIS\n\
         hello\n",
    );
}

#[test]
fn shared_string_reference_links() {
    // The same String written twice is a `@`-link the second time, so a
    // mutation through one alias is visible through the other after load.
    let result = run_ruby(
        r#"
        s = "ab"
        g = Marshal.load(Marshal.dump([s, s]))
        g[0] << "z"
        p g[1]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"abz\"\n");
}

#[test]
fn regexp_class_and_struct_wire_and_roundtrip() {
    // `/` (regexp source + option byte, `I`-wrapped for encoding), `c`/`m`
    // (class/module reference), and `S` (Struct: class symbol, member count,
    // member/value pairs).
    let result = run_ruby(
        r#"
        def wire(x) = Marshal.dump(x).bytes.join(",")
        def rt(x) = Marshal.load(Marshal.dump(x))
        puts wire(/ab.c/im)
        r = rt(/a\d+b/i)
        puts r.source
        puts r.options
        puts wire(String)
        p rt(String)
        p rt(Comparable)
        S = Struct.new(:a, :b)
        puts wire(S.new(1, 2))
        st = rt(S.new(10, "x"))
        p [st.a, st.b]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "4,8,73,47,9,97,98,46,99,5,6,58,6,69,70\n\
         a\\d+b\n\
         1\n\
         4,8,99,11,83,116,114,105,110,103\n\
         String\n\
         Comparable\n\
         4,8,83,58,6,83,7,58,6,97,105,6,58,6,98,105,7\n\
         [10, \"x\"]\n",
    );
}

#[test]
fn user_marshal_and_userdef_hooks() {
    // `U` (marshal_dump/marshal_load) and `u` (_dump/self._load) drive
    // serialization through user methods; both round-trip through allocate +
    // the hook.
    let result = run_ruby(
        r#"
        def wire(x) = Marshal.dump(x).bytes.join(",")
        def rt(x) = Marshal.load(Marshal.dump(x))
        class UBox
          def initialize(v = nil) = (@v = v)
          attr_reader :v
          def marshal_dump = [@v, 7]
          def marshal_load(a) = (@v = a[0])
        end
        puts wire(UBox.new(5))
        p rt(UBox.new("payload")).v
        class LilEndian
          def initialize(n = 0) = (@n = n)
          attr_reader :n
          def _dump(depth) = [@n].pack("N")
          def self._load(s) = new(s.unpack1("N"))
        end
        puts wire(LilEndian.new(258))
        p rt(LilEndian.new(65535)).n
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "4,8,85,58,9,85,66,111,120,91,7,105,10,105,12\n\
         \"payload\"\n\
         4,8,117,58,14,76,105,108,69,110,100,105,97,110,9,0,0,1,2\n\
         65535\n",
    );
}
