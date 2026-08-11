# A `class` written inside a lambda body defines its class where it is
# written, exactly like one inside a block -- both run later, maybe never,
# and the registration is a compile-time fact either way (the body still
# EXECUTES only when the lambda runs).
#
# zeo's registration walk descended blocks but stopped at lambdas, so the
# marker reached codegen with no site: "`class`/`module` in a position the
# analyze walk doesn't register". `class Object` reopens inside lambdas
# dominate that ledger bucket.
handler = lambda do
  class BuiltInsideALambda
    def greeting
      "from inside"
    end
  end
end

handler.call
puts BuiltInsideALambda.new.greeting

arrow = -> {
  class Object
    def lambda_patched
      "patched"
    end
  end
}
arrow.call
puts lambda_patched
