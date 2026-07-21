# Fiber[:k] = nil must compile: fiber storage is poly-valued, so a nil value
# is carried boxed (not declared as a `void` temp).
Fiber.new do
  Fiber[:db_conn] = nil
  Fiber[:db_conn] = "conn"
  puts Fiber[:db_conn]
  Fiber[:db_conn] = nil
  puts Fiber[:db_conn].nil?
end.resume
