DIR = File.expand_path("fixtures/singleton_yield", __dir__)
$LOAD_PATH.unshift(DIR)

name = "singleton_yield_target"
require name

p SingletonYield.take(2) { |v| v * 10 }
