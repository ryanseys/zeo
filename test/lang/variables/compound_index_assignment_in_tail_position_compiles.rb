# Regression test for a pre-existing codegen bug found during the
# freeze work (not freeze-related): `HirNode::Seq` -- the lowering
# artifact behind `arr[i] += 1` -- emitted a brace-less statement
# sequence from `emit_expr`, producing invalid Rust (`Ok(stmt; stmt;
# expr)`) whenever the compound assignment was the LAST statement of a
# method/`begin` body. Latent since the `Seq` arm was written; every
# prior test happened to put a statement after it.

class Bumper
  def bump
    arr = [10]
    arr[0] += 1
  end
end
puts Bumper.new.bump
__END__
11
