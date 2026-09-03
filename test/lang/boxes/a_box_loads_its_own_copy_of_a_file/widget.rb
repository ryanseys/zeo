$boxed_widget_runs = ($boxed_widget_runs || 0) + 1

class BoxedWidget
  def hi = "widget #{$boxed_widget_runs}"
end

BOXED_WIDGET_CONST = 99
