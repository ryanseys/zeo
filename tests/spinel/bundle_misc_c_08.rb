# Bundled tests:
#   - dig_nested_poly_array
#   - each_cons_map_chain_fusion
#   - each_cons_with_index_map_chain_fusion
#   - env_fetch
#   - equality_should_be_true
#   - fiber_storage_assign_expr

# === dig_nested_poly_array ===
def t_dig_nested_poly_array
  # Array#dig walks through nested poly_array elements with int keys.
  # Pre-fix emit_dig_step's int-key branch only emitted an arm for
  # SP_BUILTIN_INT_STR_HASH; every other receiver kind collapsed the
  # accumulator to nil at the next step, so a nested poly_array
  # dig surfaced nil instead of the leaf value. Issue #619 puzzle 6.
  
  p [[1, [2, "3"]]].dig(0, 1, 1) == "3"   # true (string leaf)
  p [[1, [2, "3"]]].dig(0, 0) == 1        # true (int leaf via box_int)
  p [[1, [2, "3"]]].dig(0, 1, 0) == 2     # true
  p [[1, [2, "3"]]].dig(0, 1, 5).nil?     # true (OOB at the last step)
end
t_dig_nested_poly_array

# === each_cons_map_chain_fusion ===
def t_each_cons_map_chain_fusion
  # Phase A of Enumerator-chain strategy (#566): `arr.each_cons(n)`
  # called without a block returns an Enumerator in CRuby. When the
  # very next call is a terminal like `.map { |pair| ... }`, the
  # Enumerator is consumed immediately and the chain has the same
  # observable result as eagerly materialising each window. Spinel
  # fuses the source + terminal into a single C loop, so no
  # Enumerator object is allocated and no intermediate
  # array-of-arrays is built (one window allocation per iteration,
  # bounded by .map's accumulator).
  #
  # The terminal block may take the window as a single param
  # (`|pair|`, typed as the receiver's array shape) or destructure
  # it (`|(a, b)|`, binds individual locals -- the per-iteration
  # window allocation is skipped on this path).
  
  # T1: plain |pair| form, scalar block result
  p [1, 2, 3, 4].each_cons(2).map { |pair| pair[0] + pair[1] }
  
  # T2: destructure |(a, b)|, scalar block result
  p [10, 20, 30, 40].each_cons(2).map { |(a, b)| b - a }
  
  # T3: |pair| with array-returning block -> result is int_array_ptr_array
  p [1, 2, 3].each_cons(2).map { |pair| [pair[0], pair[1]] }
  
  # T4: empty / short receiver (length < n) -> empty result
  p [1].each_cons(2).map { |pair| pair[0] }
end
t_each_cons_map_chain_fusion

# === each_cons_with_index_map_chain_fusion ===
def t_each_cons_with_index_map_chain_fusion
  # Phase A.3 of Enumerator-chain strategy (#566): three-step
  # `arr.each_cons(n).with_index(off).map { ... }` chain. CRuby
  # returns an Enumerator from each_cons, then another from
  # with_index, and consumes both at the terminal .map. Spinel
  # fuses the whole chain into a single C loop with an idx counter
  # initialised from `off` (default 0) and incremented after each
  # pair, so no Enumerator object is allocated.
  #
  # Block param shapes:
  #   |pair, i|       -- pair is the typed sub-array, i is the int idx
  #   |(a, b), i|     -- nested destructure of pair into individual
  #                      window slots (skips the per-iteration sub-
  #                      array allocation), trailing i as int idx
  
  # T1: |pair, i| with explicit offset
  p [10, 20, 30, 40].each_cons(2).with_index(1).map { |pair, i| pair[0] + pair[1] + i }
  
  # T2: |(a, b), i| destructure, explicit offset
  p [10, 20, 30, 40].each_cons(2).with_index(1).map { |(a, b), i| b - a + i }
  
  # T3: default offset (0)
  p [1, 2, 3].each_cons(2).with_index.map { |pair, i| pair[1] - pair[0] + i }
  
  # T4: block returns array -> result is int_array_ptr_array
  p [1, 2, 3, 4].each_cons(2).with_index(1).map { |(a, b), i| [b - a, i] }
end
t_each_cons_with_index_map_chain_fusion

# === env_fetch ===
def t_env_fetch
  # Regression: `ENV.fetch(key, default)` returns the env value when
  # set, otherwise `default`. Pre-fix the codegen emitted "cannot
  # resolve call to 'fetch' on int" and segfaulted at runtime.
  
  # ---- unset / default branch ----
  
  # Literal default.
  puts ENV.fetch("DEFINITELY_UNSET_VAR_XYZ_42", "fallback-value")
  
  # Default expression need not be a literal.
  default = "computed-default"
  puts ENV.fetch("ANOTHER_UNSET_VAR_XYZ_42", default)
  
  # Default by string concatenation.
  puts ENV.fetch("THIRD_UNSET_VAR_XYZ_42", "pre-" + "fix")
  
  # ---- set / retrieval branch ----
  # Spinel doesn't currently expose `ENV[]=` from Ruby, so we can't
  # set a var ourselves. HOME is always exported by POSIX `make` and
  # in CI, so we use that as the "set" probe. We only assert "non-
  # empty + not the fallback string" so the test stays portable
  # across runners (the actual home value differs per machine).
  # A regression that inverted the getenv-result ternary would
  # return the fallback here and print "set=fallback-not-used".
  home = ENV.fetch("HOME", "fallback-not-used")
  if home.length > 0 && home != "fallback-not-used"
    puts "set=ok"
  else
    puts "set=" + home
  end
  
  # Symmetric: an unset var with a literal default round-trips
  # through the same ternary. (Would catch a regression that always
  # returned the env value -- empty string -- regardless.)
  maybe = ENV.fetch("DEFINITELY_UNSET_VAR_ABC_99", "fallback-used")
  puts "unset=" + maybe
end
t_env_fetch

# === equality_should_be_true ===
def t_equality_should_be_true
  # #555 (gurgeous/Adam Doppelt). A corpus of `should be true`
  # expressions that compiled to false in spinel pre-fix.
  # Each line is an independent equality predicate; CRuby
  # returns true for all twelve, and after the followup
  # commits spinel matches on all 12.
  
  p ({} == {})                                     # 01
  p ({a: 1} == {a: 1})                             # 02
  p ({:a= => 1} == {:"a=" => 1})                   # 03
  p ({:a! => 1} == {:"a!" => 1})                   # 04
  p ({:a? => 1} == {:"a?" => 1})                   # 05
  a = [1] ; a.shift ; a << :foo ; p (a == [:foo])  # 06
  p ([42, { foo: :bar }].dig(1, :foo) == :bar)     # 07
  a2 = [1, 2, 3, 4, 5] ; p ((a2[2, 3] = 10) == 10) # 08
  p ("hello".chars == ['h', 'e', 'l', 'l', 'o'])   # 09
  p "abc\r\n".chomp(nil) == "abc\r\n"              # 10
  p ((/(.)(.)(.)/ =~ "abc") == 0)                  # 11
  p ((1..).send(:include?, 2.4) == true)           # 12
end
t_equality_should_be_true

# === fiber_storage_assign_expr ===
def t_fiber_storage_assign_expr
  # `(Fiber[:k] = v)` as an expression returns the assigned value,
  # matching MRI's `(h[k]=v) => v` semantics. The codegen path is
  # distinct from the statement form `Fiber[:k] = v` — that lands in
  # compile_mutating_call_stmt, while the expression-context dispatch
  # emits a gcc statement-expression so the RHS reaches the outer
  # assignment.
  
  x = (Fiber[:n] = 42)
  puts x                # 42 — value of the assignment expression
  puts Fiber[:n]        # 42 — confirms the write also took effect
  
  # Chained: `Fiber[:a] = Fiber[:b] = 7`. Right-to-left associativity
  # makes `Fiber[:b] = 7` evaluate first (yielding 7), then
  # `Fiber[:a] = 7`.
  Fiber[:a] = Fiber[:b] = 7
  puts Fiber[:a]
  puts Fiber[:b]
end
t_fiber_storage_assign_expr

