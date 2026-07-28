# A `class`/`module` written inside a top-level `begin` is REGISTERED by the
# analyze walk but its body still executes from the prelude, ahead of every
# ordinary top-level statement -- so a raise from the body escapes the `rescue`
# that lexically encloses it, and its `<main>` caller frame has no line.
#
# Ruby runs the body where it is written: "before" prints, the rescue catches,
# "after" prints.
puts "before"
begin
  class Registry
    Undefined
  end
rescue NameError => e
  puts "caught #{e.message}"
end
puts "after"
