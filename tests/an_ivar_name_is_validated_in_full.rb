# `instance_variable_get`/`set` validate the whole name: "@@bad" (a class
# variable spelling) raises NameError "'@@bad' is not allowed as an
# instance variable name". zeo answers nil for the get. A plain bad name
# (:novar) is already refused. (Found by the 2026-08-24 probe sweep.)
begin
  p Object.new.instance_variable_get("@@bad")
rescue NameError => e
  puts e.message
end
begin
  Object.new.instance_variable_set("@@bad", 1)
rescue NameError => e
  puts e.message
end
