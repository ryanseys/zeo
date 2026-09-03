# `break` inside an explicitly created proc invoked via `#call` has no
# iterator to unwind to, so CRuby raises LocalJumpError (`#reason`
# `:break`) -- even while the creating iterator is still live. A lambda's
# `break`, by contrast, just returns from the lambda.

p(-> { break 9 }.call)
def orphan; proc { break 1 }; end
begin
  orphan.call
rescue LocalJumpError => e
  puts "#{e.message} / #{e.reason.inspect}"
end
[1, 2].each do |x|
  pr = proc { break :pb }
  begin
    pr.call
  rescue LocalJumpError => e
    puts "live: #{e.message}"
  end
end
__END__
9
break from proc-closure / :break
live: break from proc-closure
live: break from proc-closure
