# The `io/console` argument guards, all of them io-console's own.
#
# The worst of these was `min:`: a non-numeric went through an "unchecked"
# float conversion that PANICS, so `io.raw!(min: "x")` aborted the whole
# process where ruby raises. An unknown keyword was dropped silently, and a
# stray positional argument sent `getch` into a blocking read.

require "io/console"

r, w = IO.pipe

def refusal
  yield
  "no refusal"
rescue StandardError => e
  "#{e.class}: #{e.message}"
end

# Keywords: only min:, time: and intr:, and intr: is true or false.
puts refusal { r.raw(nope: 1) { 1 } }
puts refusal { r.raw!(min: "x") }
puts refusal { r.raw!(time: "x") }
puts refusal { r.raw!(intr: 1) }

# No positional argument at all.
puts refusal { r.getch(:nope) }
puts refusal { r.raw!(1) }

# `winsize=` takes 2 or 4 elements, counted after `Kernel#Array`.
puts refusal { r.winsize = [1] }
puts refusal { r.winsize = "x" }
puts refusal { r.winsize = [1, 2, 3] }

# The erase modes are bounded, and nil means 0.
puts refusal { w.erase_line(9) }
puts refusal { w.erase_screen(4) }
puts refusal { w.erase_line("x") }

# `cursor=` wants a 2D coordinate.
puts refusal { w.cursor = [1] }
puts refusal { w.cursor = 5 }

# A block-taking row with no block.
puts refusal { r.noecho }
puts refusal { r.raw }

# `IO.console`'s argument is a Symbol.
puts refusal { IO.console("close") }

r.close
w.close
__END__
ArgumentError: unknown keyword: :nope
TypeError: no implicit conversion of String into Integer
TypeError: no implicit conversion of String into Integer
ArgumentError: true or false expected as intr: 1
ArgumentError: wrong number of arguments (given 1, expected 0)
ArgumentError: wrong number of arguments (given 1, expected 0)
ArgumentError: wrong number of arguments (given 1, expected 2 or 4)
ArgumentError: wrong number of arguments (given 1, expected 2 or 4)
ArgumentError: wrong number of arguments (given 3, expected 2 or 4)
ArgumentError: wrong line erase mode: 9
ArgumentError: wrong screen erase mode: 4
ArgumentError: wrong line erase mode: x
ArgumentError: expected 2D coordinate
TypeError: no implicit conversion of Integer into Array
Errno::ENOTTY: Inappropriate ioctl for device
Errno::ENOTTY: Inappropriate ioctl for device
TypeError: wrong argument type String (expected Symbol)
