# Loaded by an `autoload` in tests/an_autoload_runs_when_its_constant_is_read.rb.
$autoload_widget_loads = ($autoload_widget_loads || 0) + 1

module ByFeature
  class Widget
    def name = "widget"
  end
end
