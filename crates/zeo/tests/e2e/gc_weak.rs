//! `ObjectSpace::WeakMap`, `ObjectSpace.define_finalizer`, `GC`, and `WeakRef`.
//! Every expectation is pinned to the ruby 4.0.6 oracle
//! (`--disable-error_highlight --disable-did_you_mean`). Weak-reference tests
//! avoid pinning non-deterministic addresses and only assert liveness/pruning.

use crate::support::run_ruby;

#[test]
fn weakmap_full_api_and_identity_keying() {
    let result = run_ruby(
        r#"
        m = ObjectSpace::WeakMap.new
        p m.class
        k1 = "a"; k2 = "b"
        v1 = Object.new; v2 = Object.new
        m[k1] = v1
        m[k2] = v2
        p m[k1].equal?(v1)
        p m.key?(k1)
        p m.include?(k2)
        p m.member?("absent")
        p m.length
        p m.size
        p m.keys.length
        p m.values.length
        # identity keying: an equal-but-different key string misses
        p m["a"].nil?
        # overwrite in place, not a second entry
        m[k1] = v2
        p m.length
        p m[k1].equal?(v2)
        # delete returns the value, then the key is gone
        p m.delete(k1).equal?(v2)
        p m[k1].nil?
        p m.length
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "ObjectSpace::WeakMap\ntrue\ntrue\ntrue\nfalse\n2\n2\n2\n2\ntrue\n\
         2\ntrue\ntrue\ntrue\n1\n"
    );
}

#[test]
fn weakmap_iteration_and_inspect() {
    let result = run_ruby(
        r##"
        m = ObjectSpace::WeakMap.new
        a = Object.new; b = Object.new
        m[a] = 1
        m[b] = 2
        vals = []
        m.each { |k, v| vals << v }
        p vals.sort
        keys_seen = 0
        m.each_key { |k| keys_seen += 1 }
        p keys_seen
        vs = []
        m.each_value { |v| vs << v }
        p vs.sort
        pairs = 0
        m.each_pair { |k, v| pairs += 1 }
        p pairs
        # empty map inspect starts with the byte-exact CRuby prefix
        empty = ObjectSpace::WeakMap.new
        p(empty.inspect.start_with?("#<ObjectSpace::WeakMap:0x"))
        p empty.inspect.end_with?(">")
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[1, 2]\n2\n[1, 2]\n2\ntrue\ntrue\n");
}

#[test]
fn weakmap_prunes_a_collected_key_at_gc() {
    // A key with no other strong reference is pruned once collected.
    let result = run_ruby(
        r#"
        m = ObjectSpace::WeakMap.new
        m[Object.new] = "gone"
        GC.start
        p m.keys.length
        p m.length
        # an immediate key is held strongly and never expires; keep the value
        # strongly referenced so only key liveness is under test
        m2 = ObjectSpace::WeakMap.new
        val = Object.new
        m2[42] = val
        GC.start
        p m2[42].equal?(val)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "0\n0\ntrue\n");
}

#[test]
fn objectspace_module_surface_and_honest_not_implemented() {
    let result = run_ruby(
        r#"
        p ObjectSpace.count_objects.class
        p ObjectSpace.garbage_collect
        begin
          ObjectSpace.each_object(String) {}
        rescue NotImplementedError => e
          puts "each_object: #{e.message}"
        end
        begin
          ObjectSpace._id2ref(8)
        rescue NotImplementedError => e
          puts "_id2ref: #{e.message}"
        end
        # GC surface stays no-op-but-shaped
        p GC.start
        p GC.count.class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Hash\nnil\n\
         each_object: ObjectSpace.each_object is not available (zeo has no heap enumeration)\n\
         _id2ref: ObjectSpace._id2ref is not available (zeo has no id-to-object table)\n\
         nil\nInteger\n"
    );
}

/// The introspection half that zeo declines: each error names the capability
/// it would need, so a caller learns why rather than reading a fabricated
/// zero. `tests/objspace_introspection.rb` covers the half that does answer.
#[test]
fn objspace_declines_name_the_missing_capability() {
    let result = run_ruby(
        r#"
        require "objspace"
        %i[
          memsize_of_all reachable_objects_from_root
          trace_object_allocations_start trace_object_allocations_stop
          dump dump_all dump_shapes internal_class_of internal_super_of
        ].each do |m|
          begin
            ObjectSpace.public_send(m, "x")
          rescue NotImplementedError => e
            puts e.message
          end
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "ObjectSpace.memsize_of_all is not available (zeo has no heap enumeration)\n\
         ObjectSpace.reachable_objects_from_root is not available (zeo has no GC root table)\n\
         ObjectSpace.trace_object_allocations_start is not available (zeo has no allocation hook)\n\
         ObjectSpace.trace_object_allocations_stop is not available (zeo has no allocation hook)\n\
         ObjectSpace.dump is not available (zeo objects carry no VM header to serialize)\n\
         ObjectSpace.dump_all is not available (zeo has no heap enumeration)\n\
         ObjectSpace.dump_shapes is not available (zeo has no shape tree)\n\
         ObjectSpace.internal_class_of is not available (zeo has no internal classes)\n\
         ObjectSpace.internal_super_of is not available (zeo has no internal classes)\n"
    );
}

#[test]
fn define_finalizer_runs_at_exit_and_validates_args() {
    // The finalizer for a still-referenced object runs at program exit; the
    // undefined one never runs; the bad-argument shapes match CRuby.
    let result = run_ruby(
        r#"
        s = Object.new
        ObjectSpace.define_finalizer(s, proc { |id| puts "finalized(#{id.class})" })
        t = Object.new
        ObjectSpace.define_finalizer(t) { |id| puts "block finalizer" }
        u = Object.new
        ObjectSpace.define_finalizer(u, proc { puts "SHOULD NOT RUN" })
        ObjectSpace.undefine_finalizer(u)
        begin
          ObjectSpace.define_finalizer("x")
        rescue ArgumentError => e
          puts "e1: #{e.message}"
        end
        begin
          ObjectSpace.define_finalizer("x", 5)
        rescue ArgumentError => e
          puts "e2: #{e.message}"
        end
        puts "before exit"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "e1: tried to create Proc object without a block\n\
         e2: wrong type argument Integer (should be callable)\n\
         before exit\n\
         finalized(Integer)\n\
         block finalizer\n"
    );
}

#[test]
fn define_finalizer_runs_at_gc_for_a_collected_object() {
    let result = run_ruby(
        r#"
        def register = ObjectSpace.define_finalizer(Object.new, proc { |id| puts "collected" })
        register
        GC.start
        puts "after gc"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "collected\nafter gc\n");
}

#[test]
fn weakref_delegates_to_a_live_referent() {
    let result = run_ruby(
        r#"
        require "weakref"
        s = "hello"
        w = WeakRef.new(s)
        p w.class
        p w.upcase
        p w.__getobj__
        p w.weakref_alive?
        p w.respond_to?(:upcase)
        p w.respond_to?(:no_such_method)
        obj = Object.new
        def obj.greet(name) = "hi, #{name}"
        w2 = WeakRef.new(obj)
        p w2.greet("world")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "WeakRef\n\"HELLO\"\n\"hello\"\ntrue\ntrue\nfalse\n\"hi, world\"\n"
    );
}

#[test]
fn weakref_raises_ref_error_once_collected() {
    let result = run_ruby(
        r#"
        require "weakref"
        def make = WeakRef.new(Object.new)
        w = make
        GC.start
        p w.weakref_alive?
        begin
          w.some_method
        rescue WeakRef::RefError => e
          puts "RefError: #{e.message}"
        end
        begin
          w.__getobj__
        rescue WeakRef::RefError => e
          puts "getobj: #{e.class}"
        end
        # WeakRef::RefError is a StandardError, so a bare rescue catches it
        p WeakRef::RefError.superclass
        p WeakRef::RefError.ancestors.include?(StandardError)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "false\n\
         RefError: Invalid Reference - probably recycled\n\
         getobj: WeakRef::RefError\n\
         StandardError\ntrue\n"
    );
}
