use crate::support::{run_ruby};

#[test]
fn compound_index_assignment_in_tail_position_compiles() {
    // Regression test for a pre-existing codegen bug found during the
    // freeze work (not freeze-related): `HirNode::Seq` -- the lowering
    // artifact behind `arr[i] += 1` -- emitted a brace-less statement
    // sequence from `emit_expr`, producing invalid Rust (`Ok(stmt; stmt;
    // expr)`) whenever the compound assignment was the LAST statement of a
    // method/`begin` body. Latent since the `Seq` arm was written; every
    // prior test happened to put a statement after it.
    let result = run_ruby(
        r#"
        class Bumper
          def bump
            arr = [10]
            arr[0] += 1
          end
        end
        puts Bumper.new.bump
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "11\n");
}

#[test]
fn random_is_seeded_reproducible_and_typed() {
    let result = run_ruby(
        r#"
        p(Random.new(42).rand(1000) == Random.new(42).rand(1000))
        p(Random.new(1).rand(1000) != Random.new(2).rand(1000))
        p Random.new(5).rand(10).class
        p Random.new(5).rand(2.5).class
        p Random.new(5).rand.class
        p((r = Random.new(9).rand(6)) >= 0 && r < 6)
        p Random.new(1).bytes(8).bytesize
        p Random.new(123).seed
        p Random.new(3.9).seed
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\ntrue\nInteger\nFloat\nFloat\ntrue\n8\n123\n3\n"
    );
}
