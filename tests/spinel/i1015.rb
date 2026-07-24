def check(selector, body)
  tag = selector[/\A[a-z]+/]
  raise "no #{selector}" unless tag && body.include?("<#{tag}")
  yield
end
check("div", "<div>x</div>") { puts "found div" }
begin
  check("p", "<div>x</div>") { puts "unreachable" }
rescue => e
  puts "raised: #{e.message}"
end
