# sleep with a poly argument (a duration pulled from a heterogeneous hash)
# must coerce poly -> float for sp_sleep(mrb_float).
h = { "t" => 0.0, "n" => 2 }
sleep(h["t"])   # poly value, holds a Float
puts "slept"
