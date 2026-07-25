//! `SecureRandom` (built-in shim over the native `Random::Formatter` mixin and
//! OS entropy), and the `extend`-onto-class/module capability it rests on.
//!
//! Random output can't be pinned to exact bytes, so these tests assert the
//! observable CONTRACT -- lengths, formats, ranges, encoding, and the fact that
//! successive draws differ (a seeded PRNG would fail the last one). The
//! `extend` tests pin the language capability directly.

use crate::support::run_ruby;

#[test]
fn securerandom_formatted_lengths_and_types() {
    let result = run_ruby(
        r#"
        require "securerandom"
        puts SecureRandom.hex(8).length          # 2*n
        puts SecureRandom.hex.length             # default n=16
        puts SecureRandom.hex(8).class
        puts SecureRandom.base64(6).length        # 4/3 * n, padded
        puts SecureRandom.random_bytes(5).bytesize
        puts SecureRandom.random_bytes(5).encoding.to_s
        puts SecureRandom.alphanumeric(12).length
        puts(SecureRandom.alphanumeric(20) =~ /\A[A-Za-z0-9]{20}\z/ ? "alnum_ok" : "alnum_BAD")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "16\n32\nString\n8\n5\nASCII-8BIT\n12\nalnum_ok\n"
    );
}

#[test]
fn securerandom_uuid_is_a_valid_v4() {
    let result = run_ruby(
        r#"
        require "securerandom"
        u = SecureRandom.uuid
        puts(u =~ /\A\h{8}-\h{4}-4\h{3}-[89ab]\h{3}-\h{12}\z/ ? "uuid_ok" : "uuid_BAD: #{u}")
        puts(SecureRandom.uuid_v4 =~ /\A\h{8}-\h{4}-4\h{3}-[89ab]\h{3}-\h{12}\z/ ? "v4_ok" : "v4_BAD")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "uuid_ok\nv4_ok\n");
}

#[test]
fn securerandom_urlsafe_base64_padding_rules() {
    let result = run_ruby(
        r#"
        require "securerandom"
        # Default: no padding, URL/filename-safe alphabet only. n=5 isn't a
        # multiple of 3, so a padded encoding really does carry a trailing '='.
        s = SecureRandom.urlsafe_base64(5)
        puts(s =~ /\A[A-Za-z0-9\-_]+\z/ ? "urlsafe_ok" : "urlsafe_BAD: #{s}")
        puts s.include?("=")
        # Explicit padding: keeps the '='.
        puts SecureRandom.urlsafe_base64(5, true).include?("=")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "urlsafe_ok\nfalse\ntrue\n");
}

#[test]
fn securerandom_random_number_bounds() {
    let result = run_ruby(
        r#"
        require "securerandom"
        puts SecureRandom.random_number(10).class
        puts (0...200).all? { SecureRandom.random_number(10) < 10 }
        puts (0...200).all? { n = SecureRandom.random_number(1); n == 0 }   # [0,1) integer => always 0
        puts SecureRandom.random_number.class                              # no arg => Float in [0,1)
        puts (0.0...1.0).include?(SecureRandom.random_number)
        puts SecureRandom.random_number(1.5).class                         # positive Float => Float
        # Range form
        r = SecureRandom.random_number(5..9)
        puts (5..9).include?(r)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Integer\ntrue\ntrue\nFloat\ntrue\nFloat\ntrue\n"
    );
}

#[test]
fn securerandom_is_cryptographic_not_a_seeded_prng() {
    // Two independent draws must differ, and -- unlike Kernel#rand -- srand must
    // NOT make SecureRandom reproducible (it reads the OS CSPRNG, not the seedable
    // generator). A regression to a clock-seeded xorshift would break this.
    let result = run_ruby(
        r#"
        require "securerandom"
        puts(SecureRandom.hex(16) != SecureRandom.hex(16))
        srand(42); a = SecureRandom.hex(16)
        srand(42); b = SecureRandom.hex(16)
        puts(a != b)
        # Random.urandom itself: correct bytesize, drawn fresh each call.
        puts Random.urandom(24).bytesize
        puts(Random.urandom(8) != Random.urandom(8))
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\n24\ntrue\n");
}

// --- The `extend`-onto-class/module capability SecureRandom rests on. ---

#[test]
fn extend_module_with_native_module_installs_class_methods() {
    // `SomeModule.extend(Comparable)` -- a native module's methods become the
    // receiver's own module methods, running with the module as `self`.
    let result = run_ruby(
        r#"
        module Wrapper
          def self.<=>(other); 0; end
        end
        Wrapper.extend(Comparable)
        puts Wrapper.respond_to?(:clamp)
        puts Wrapper.between?(Wrapper, Wrapper)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\n");
}

#[test]
fn extend_class_with_user_module_makes_class_methods() {
    let result = run_ruby(
        r#"
        module Greeter
          def hello; "hello from #{name}"; end
        end
        class Widget
          def self.name; "Widget"; end
          extend Greeter
        end
        # And the runtime call form on a plain class:
        class Gadget; end
        module M; def tag; "tagged"; end; end
        Gadget.extend(M)
        puts Widget.hello
        puts Gadget.tag
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hello from Widget\ntagged\n");
}

#[test]
fn extend_module_with_user_module_via_runtime_call() {
    let result = run_ruby(
        r#"
        module Sayer
          def say; "said"; end
        end
        module Speaker; end
        Speaker.extend(Sayer)
        puts Speaker.say
        puts Speaker.respond_to?(:say)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "said\ntrue\n");
}

#[test]
fn extended_formatter_redispatches_gen_random_to_the_host() {
    // The critical semantic behind `Gem::SecureRandom`: a host defines its own
    // `gen_random` leaf, extends the native formatter, and the formatter's
    // `hex`/`random_bytes` redispatch back to that leaf -- so a DETERMINISTIC
    // leaf makes the formatted output deterministic and testable exactly.
    let result = run_ruby(
        r#"
        require "random/formatter"
        # `Random::Formatter` is a NATIVE module, so it must be mixed in with the
        # runtime `extend` call (the in-body directive only re-materializes a
        # user module's own Ruby methods).
        module FixedSource
          def self.gen_random(n); ("\xAB".b * n); end
        end
        FixedSource.extend(Random::Formatter)
        module FixedSource2
          def self.gen_random(n); ("\x00".b * n); end
        end
        FixedSource2.extend(Random::Formatter)

        puts FixedSource.hex(4)                 # "abababab"
        puts FixedSource.random_bytes(3).bytes.inspect
        puts FixedSource2.hex(4)                # "00000000"
        puts FixedSource2.random_number(256)    # first byte of 0x00.. => 0
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "abababab\n[171, 171, 171]\n00000000\n0\n"
    );
}

#[test]
fn own_singleton_method_outranks_an_extended_module() {
    // CRuby ancestry: a receiver's own `def self.x` beats a module it extends.
    let result = run_ruby(
        r#"
        module Mixin
          def label; "from mixin"; end
        end
        module Host
          def self.label; "from host"; end
        end
        Host.extend(Mixin)
        puts Host.label
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "from host\n");
}

#[test]
fn extending_a_non_module_raises_type_error() {
    let result = run_ruby(
        r#"
        begin
          Object.new.extend(42)
        rescue TypeError => e
          puts "TypeError"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "TypeError\n");
}
