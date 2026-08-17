t = Thread.new { Thread.stop }
sleep 0.1
p t.status
t.run
t.kill
t.join
p t.status
