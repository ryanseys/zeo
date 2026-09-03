# Three lowering gaps that CRuby resolves differently than a naive compiler
# would: an explicit `cause:`, the `$+` backreference, and adjacent
# interpolated string literals.

# 1. `raise ..., cause:` is genuinely THREE-state. An omitted cause: chains
#    automatically from $!; `cause: nil` SUPPRESSES that chaining; a value
#    installs it. The first two mean opposite things, so they cannot share a
#    representation.
def blow(m); raise "top", cause: ArgumentError.new(m); end
begin
  blow("root")
rescue => e
  p e.cause
  puts e.message
end

begin
  begin
    raise ArgumentError, "inner"
  rescue ArgumentError
    raise "outer"
  end
rescue => e
  p e.cause          # implicit chaining still applies
end

begin
  begin
    raise ArgumentError, "inner2"
  rescue ArgumentError
    raise "outer2", cause: nil
  end
rescue => e
  p e.cause          # nil -- explicitly suppressed
end

begin
  raise "x", cause: 5
rescue TypeError => e
  puts "TypeError: #{e.message}"
end

begin
  a = RuntimeError.new("a")
  b = RuntimeError.new("b")
  begin
    raise a, cause: b
  rescue; end
  raise b, cause: a
rescue ArgumentError => e
  puts "ArgumentError: #{e.message}"
end

# 2. `$+` is the highest-numbered group that actually PARTICIPATED, skipping
#    declared-but-unmatched ones, and is nil when only group 0 matched.
"abc123" =~ /([a-z]+)(\d+)/
puts $1
puts $2
puts $+
"abc" =~ /([a-z]+)(\d+)?/
puts $1
puts $+           # steps back to the group that did match
"xyz" =~ /xyz/
p $+              # nil -- $+ never reports the whole match
"b" =~ /(a)|(b)|(c)/
p $+

# 3. Backslash-continued adjacent literals parse as an interpolated string
#    whose own parts are interpolated strings, so parts must flatten.
def svg(px, inner)
  "<a width='#{px}' " \
  "height='#{px}'>#{inner}</a>"
end
def three(a, b)
  "[" \
  "#{a}-#{b}" \
  "]"
end
def tail(n)
  "n=#{n}" \
  " done"
end
puts svg(16, "x")
puts three("L", "R")
puts tail(7)

__END__
#<ArgumentError: root>
top
#<ArgumentError: inner>
nil
TypeError: exception object expected
ArgumentError: circular causes
abc
123
123
abc
abc
nil
"b"
<a width='16' height='16'>x</a>
[L-R]
n=7 done
