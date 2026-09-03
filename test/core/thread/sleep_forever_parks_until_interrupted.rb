# `sleep` with no duration parks until an interrupt arrives, then delivers
# it normally.

Thread.report_on_exception = false
t = Thread.new do
  begin
    sleep
    :never
  rescue => e
    "woken: #{e.message}"
  end
end
sleep 0.05
t.raise("done sleeping")
p t.value
__END__
"woken: done sleeping"
