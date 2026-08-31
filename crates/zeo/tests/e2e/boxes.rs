//! `Ruby::Box` isolation, the channel at a time.
//!
//! A box is a private copy of the class world: code inside it may reopen
//! `Array`, define constants and set class ivars, and NONE of that may reach
//! main -- while the box still sees every SHARED definition it did not
//! override. Every expectation below was taken from ruby 4.0.6 under
//! `RUBY_BOX=1` rather than reasoned out, because the two halves of each
//! pair (what leaks, what is still visible) are easy to get backwards.
//!
//! CRuby implements this with one copy-on-write record per (class, box) --
//! `rb_classext_t`, holding the method tables, the constant table, the class
//! ivars and the superclass together (`internal/class.h`). zeo reaches the
//! same observable rule per channel: a class-method and value-method row
//! carries the box it was written in, and a class the box OWNS answers its
//! own rows to every caller. The channels that still lack the axis have
//! their own tests, in `crates/zeo/tests/gaps/`.

use crate::support::{run_ruby_boxed, run_ruby_project_boxed};

/// The class-method channel. A box's `def self.x` on a SHARED class is the
/// box's alone: main sees neither the method nor a `respond_to?` for it,
/// while the box still reaches `Array`'s ordinary rows.
///
/// This is the shape that leaked -- `class_methods` was keyed by name only,
/// where the instance channel had carried `(box, name)` from the start.
#[test]
fn a_boxs_class_method_on_a_shared_class_stays_in_the_box() {
    let result = run_ruby_boxed(
        r#"
        b = Ruby::Box.new
        b.eval("class Array; def self.zzz = 9; end")
        p Array.respond_to?(:zzz)
        p(begin; Array.zzz; rescue NoMethodError; :nome; end)
        p b.eval("Array.zzz")
        p b.eval("Array.respond_to?(:zzz)")
        p b.eval("Array.new(2, 7)")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "false\n:nome\n9\ntrue\n[7, 7]\n");
}

/// The same isolation through a `class << self` body rather than a
/// `def self.x`, which reaches the class-method channel by another route.
#[test]
fn a_boxs_singleton_class_body_stays_in_the_box() {
    let result = run_ruby_boxed(
        r#"
        b = Ruby::Box.new
        b.eval("class Hash; class << self; def qq = 1; end; end")
        p Hash.respond_to?(:qq)
        p b.eval("Hash.qq")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "false\n1\n");
}

/// The instance channel, which had the box axis already -- kept here so the
/// pair reads together and a regression on either half is one file to look
/// at.
#[test]
fn a_boxs_instance_method_on_a_shared_class_stays_in_the_box() {
    let result = run_ruby_boxed(
        r#"
        b = Ruby::Box.new
        b.eval("class String; def shout = 'S'; end")
        p "a".respond_to?(:shout)
        p(begin; "a".shout; rescue NoMethodError; :nome; end)
        p b.eval("'a'.shout")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "false\n:nome\n\"S\"\n");
}

/// The OTHER direction, and the one a strict reading of the rule gets
/// wrong: a class the box DEFINED is the box's own class, so its methods
/// answer every caller, main included. Isolation is a property of a shared
/// class the box patched, never of a class it owns outright.
#[test]
fn a_class_the_box_owns_answers_main() {
    let result = run_ruby_boxed(
        r#"
        b = Ruby::Box.new
        b.eval("class Owned; def self.go = 'go'; def inst = 'i'; end")
        p b::Owned.go
        p b::Owned.new.inst
        p b::Owned.respond_to?(:go)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"go\"\n\"i\"\ntrue\n");
}

/// Two boxes patching one shared class do not see each other, and main sees
/// neither. One box could be made to work by accident (any box-shaped key
/// answers); two cannot.
#[test]
fn two_boxes_patch_one_shared_class_independently() {
    let result = run_ruby_boxed(
        r#"
        a = Ruby::Box.new
        b = Ruby::Box.new
        a.eval("class Array; def self.who = 'a'; end")
        b.eval("class Array; def self.who = 'b'; end")
        p a.eval("Array.who")
        p b.eval("Array.who")
        p(begin; Array.who; rescue NoMethodError; :nome; end)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"a\"\n\"b\"\n:nome\n");
}

/// A module reopened inside a box keeps its methods there -- `Kernel` is the
/// sharpest case, since a leak would give every object in main a new method.
#[test]
fn a_boxs_kernel_method_is_not_callable_in_main() {
    let result = run_ruby_boxed(
        r#"
        b = Ruby::Box.new
        b.eval("module Kernel; def kmeth = 1; end")
        p(begin; kmeth; rescue NoMethodError, NameError; :nome; end)
        p b.eval("kmeth")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, ":nome\n1\n");
}

/// A box's patch of a shared class must not follow a value BACK to main:
/// the Array built inside the box is an ordinary Array in main, and calling
/// the box's method on it raises there.
#[test]
fn a_value_built_in_a_box_carries_no_box_methods_into_main() {
    let result = run_ruby_boxed(
        r#"
        b = Ruby::Box.new
        b.eval("class Array; def self.zzz = 9; def mine = 'm'; end")
        a = b.eval("[1, 2]")
        p a
        p a.class == Array
        p(begin; a.mine; rescue NoMethodError; :nome; end)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[1, 2]\ntrue\n:nome\n");
}

/// A box loading a FILE (rather than an eval'd string) isolates the same
/// way. The two routes reach the compiler differently -- a literal
/// `box.eval` is spliced at compile time and a `require_relative` brings in
/// a whole unit -- so both need saying.
#[test]
fn a_boxs_required_file_patches_a_shared_class_privately() {
    let result = run_ruby_project_boxed(
        &[
            (
                "patch.rb",
                "class Array\n  def self.zzz = 9\nend\nclass Owned\n  def self.go = 'go'\nend\n",
            ),
            (
                "main.rb",
                "box = Ruby::Box.new\n\
                 box.require_relative \"patch\"\n\
                 p Array.respond_to?(:zzz)\n\
                 p box::Owned.go\n",
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "false\n\"go\"\n");
}

/// The CONSTANT channel, both halves. A box's `Object.const_set` is
/// unreachable from main, and reachable from the box -- which needs its own
/// record for `Object`, since a box's cref chain ends at its surrogate and
/// otherwise never consults `Object` at all.
#[test]
fn a_boxs_constant_is_its_own() {
    let result = run_ruby_boxed(
        r#"
        b = Ruby::Box.new
        b.eval("Object.const_set(:BOXCONST, 5)")
        p(begin; Object.const_get(:BOXCONST); rescue NameError; :namee; end)
        p b.eval("BOXCONST")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, ":namee\n5\n");
}

/// A constant written on a SHARED class inside a box, read back by path.
/// `Array::BOXC` is a scope-operator read rather than a cref walk, so it is
/// a different path into the same table.
#[test]
fn a_boxs_constant_on_a_shared_class_is_its_own() {
    let result = run_ruby_boxed(
        r#"
        b = Ruby::Box.new
        b.eval("class Array; BOXC = 11; end")
        p(begin; Array::BOXC; rescue NameError; :namee; end)
        p b.eval("Array::BOXC")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, ":namee\n11\n");
}

/// Class ivars and class variables, which live in tables of their own.
#[test]
fn a_boxs_class_ivar_and_class_variable_are_its_own() {
    let result = run_ruby_boxed(
        r#"
        b = Ruby::Box.new
        b.eval("class Array; @civ = 7; def self.civ = @civ; end")
        b.eval("class Array; @@cv = 3; def self.cv = @@cv; end")
        p(begin; Array.civ; rescue NoMethodError; :nome; end)
        p b.eval("Array.civ")
        p b.eval("Array.cv")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, ":nome\n7\n3\n");
}

/// A RUN-TIME `define_method` on a shared class's singleton. The compiled
/// half and this one land in different tables -- the frozen registry and the
/// runtime overlay -- so both need saying.
#[test]
fn a_boxs_runtime_singleton_define_method_stays_in_the_box() {
    let result = run_ruby_boxed(
        r#"
        b = Ruby::Box.new
        b.eval("Array.singleton_class.define_method(:dyn) { 42 }")
        p Array.respond_to?(:dyn)
        p b.eval("Array.dyn")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "false\n42\n");
}

/// A class-method ALIAS written in a box's `class << self`. This ABORTED the
/// process before: the alias named the box's overlay class, which registers
/// no entry, and the registrar's `expect` on that lookup is reached across an
/// `extern "C"` boundary that cannot unwind.
#[test]
fn a_boxs_singleton_alias_is_its_own_and_does_not_abort() {
    let result = run_ruby_boxed(
        r#"
        b = Ruby::Box.new
        b.eval("class Array; class << self; alias_method :zz2, :new; end; end")
        p Array.respond_to?(:zz2)
        p b.eval("Array.zz2(1, 5)")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "false\n[5]\n");
}

/// `remove_method` in a box's `class << self` retires the box's own row and
/// leaves main's alone. The removal has to find the row first -- it asked
/// about the overlay id and was told the method did not exist.
#[test]
fn a_box_removes_only_its_own_class_method() {
    let result = run_ruby_boxed(
        r#"
        b = Ruby::Box.new
        b.eval("class String; def self.zz = 1; end")
        b.eval("class String; class << self; remove_method :zz; end; end")
        p(begin; b.eval("String.zz"); rescue NoMethodError; :nome; end)
        p String.respond_to?(:zz)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, ":nome\nfalse\n");
}

/// Reflection agrees with dispatch. A name the box defined must not appear
/// in main's `singleton_methods`, which reads a name set with no box axis of
/// its own.
#[test]
fn a_boxs_class_method_is_absent_from_mains_reflection() {
    let result = run_ruby_boxed(
        r#"
        b = Ruby::Box.new
        b.eval("class Array; def self.zzz = 1; end")
        p Array.singleton_methods(false).include?(:zzz)
        p b.eval("Array.singleton_methods(false).include?(:zzz)")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "false\ntrue\n");
}

/// `include` from inside a box reaches the class's ancestry rather than its
/// own tables, and stays the box's.
#[test]
fn a_boxs_include_is_invisible_to_main() {
    let result = run_ruby_boxed(
        r#"
        b = Ruby::Box.new
        b.eval("module BoxMix; def mixed = 'm'; end; class Array; include BoxMix; end")
        p [].respond_to?(:mixed)
        p b.eval("[].mixed")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "false\n\"m\"\n");
}

/// `prepend`, which has to beat the builtin row for the box and leave main's
/// alone.
///
/// Deliberately its OWN box, not the one above. An `include` FOLLOWED BY a
/// prepend in the same box loses the prepend -- a real divergence, filed as
/// `tests/gaps/a_box_prepend_after_an_include_is_lost.rb`. Main gets that
/// pair right, so it is the box path specifically.
#[test]
fn a_boxs_prepend_is_invisible_to_main() {
    let result = run_ruby_boxed(
        r#"
        b = Ruby::Box.new
        b.eval("module BoxPre; def size = 99; end; class Array; prepend BoxPre; end")
        p [1, 2].size
        p b.eval("[1, 2].size")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n99\n");
}

/// A box minted at RUN TIME, whose id the compiler never saw. Its constant
/// has to be written and read under the same box, which is the case that
/// broke when the write learned the box before the read did.
#[test]
fn a_run_time_box_reads_back_its_own_constant() {
    let result = run_ruby_boxed(
        r#"
        d = [Ruby::Box.new].first
        d.eval("RUNTIME_BOX_CONST = 7")
        p d.eval("RUNTIME_BOX_CONST")
        p defined?(RUNTIME_BOX_CONST)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\nnil\n");
}
