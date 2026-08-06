# `autoload :Const, "#{__dir__}/sibling"` -- how puma, rack and sidekiq name a
# sibling file. zeo splices the target at compile time, so the interpolation
# has to fold then; only a plain literal and `File.expand_path` did.

autoload :Sprocket, "#{__dir__}/autoload_interpolated_dir_feature/sprocket"
autoload :Widget, "#{__dir__}/autoload_interpolated_dir_feature/sprocket"

p Sprocket.size
p Widget::NAME
p Sprocket.is_a?(Class)
