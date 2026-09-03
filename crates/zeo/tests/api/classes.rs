use crate::support::{compile_project, run_ruby};

// --- Full MRO (include/extend/prepend), inherited ivars, class
// variables, minimal raise/exception foundation -- oracle-verified against
// real `ruby` first, per this project's established convention.

// --- Bugs found via a comprehensive sweep (fixed, not deferred) ---
//
// Found by testing constructs adjacent to earlier work, not
// by design review -- each is a real, previously-undetected defect, not a
// documented scope-cut. Every test here was run against real `ruby` first,
// per this project's established convention.

// -- Correctness fixes (reopening, bare-super forwarding, cycle
// guards, dup/clone). Every positive expectation below is oracle-verified
// against real ruby 4.0.6.

// -- First-class Class/Module values. Every expectation
// oracle-verified against real ruby 4.0.6.

// ---------------------------------------------------------------------------
// Builtin-class reopening (root). Every expectation below was
// oracle-verified against real `ruby` (4.0.6) before being written down.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// The CRuby-exact builtin hierarchy (BasicObject/Kernel/
// Numeric/Rational/Complex/Math/Struct/Enumerator in the ABI; declarative
// superclass/includes seeding). Oracle: ruby 4.0.6.
// ---------------------------------------------------------------------------

#[test]
fn a_bare_opened_class_cannot_be_reparented_by_a_later_reopen() {
    // A bare `class Sub` gives Sub the class Object as its parent, so a later
    // `class Sub < Base` conflicts with a decision already made -- ruby's own
    // TypeError, raised where the reopen stands. (zeo's forward SHELLS, minted
    // for a name a later file defines, carry no such history and ARE
    // established by the reopen that declares them.)
    let result = run_ruby(
        r#"
        module M
          class Base
            def run; hook ? "blocked" : "ok"; end
            def hook; false; end
          end
          class Sub
            class Nested
              def z; 1; end
            end
          end
          begin
            class Sub < Base
              def extra; "x"; end
            end
          rescue TypeError => e
            puts e.message
          end
        end
        puts M::Sub.superclass
        puts M::Sub::Nested.new.z
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "superclass mismatch for class Sub\nObject\n1\n"
    );

    // A genuine conflict is still rejected -- as the `TypeError` ruby raises
    // at the definition, which leaves the first parent standing.
    let result = run_ruby(
        r#"
        class A; end
        class B; end
        class Sub < A; end
        begin
          class Sub < B; end
        rescue TypeError => e
          puts e.message
        end
        puts Sub.superclass
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "superclass mismatch for class Sub\nA\n");
}

/// Reopening a constant that holds a runtime class normally lowers to a runtime
/// reopen, but a body the block form can't express falls back to the STATIC
/// path rather than erroring. The shape that forced this: a constant alias to a
/// builtin (`INT_ALIAS = 1.class; class INT_ALIAS; include M; end`, the
/// to_words gem's pattern) briefly stopped compiling at all -- a worse failure
/// than the wrong answer it already had.
#[test]
fn reopening_a_runtime_class_falls_back_rather_than_failing_to_compile() {
    let compiles = |src: &str| compile_project(&[("main.rb", src)], "main.rb", &[]).is_ok();
    assert!(compiles(
        "module M; end\nFoo = Class.new\nclass Foo\n  include M\nend\n"
    ));
    assert!(compiles(
        "y = 99\nFoo = Class.new\nclass Foo\n  y = 1\nend\n"
    ));
    assert!(compiles(
        "Foo = Class.new\nclass Foo\n  private\n  def h; 1; end\nend\n"
    ));
    // The runtime path is still taken when the body IS expressible -- a
    // generated Data reader has to resolve in the reopened body.
    let result = run_ruby(
        "D = Data.define(:x)\nclass D\n  def double; x * 2; end\nend\np D.new(3).double\n",
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n");
}
