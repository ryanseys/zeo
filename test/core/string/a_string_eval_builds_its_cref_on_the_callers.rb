# A string `class_eval`/`module_eval` builds its cref on the CALLER's
# lexical chain -- CRuby pushes the receiver onto the cref of the frame the
# call is written in, so a constant beside the CALL SITE resolves inside
# the snippet. rss's dublincore writes `class DublinCoreTitle < Element`
# in a module_eval string and Element lives two scopes out.
module Outer
  class Element
    def self.tag = :outer_element
  end
  module Model; end
  Model.module_eval("p Module.nesting")
  Model.module_eval("class Kid < Element; end", __FILE__, __LINE__)
end
p Outer::Model::Kid.superclass
p Outer::Model::Kid.tag

module Outer
  module Deep
    Outer::Model.module_eval("p Module.nesting")
  end
end

def indirect_eval(mod, src)
  mod.class_eval(src)
end
module Outer
  module Deep
    E2 = Element
  end
end
p Outer::Deep.class_eval("E2") == Outer::Element

# A method's OWN lexical scope is the cref an eval written in it uses,
# wherever it is called from.
module Outer
  module Deep
    def self.probe = Outer::Model.module_eval("Module.nesting")
  end
end
p Outer::Deep.probe
__END__
[Outer::Model, Outer]
Outer::Element
:outer_element
[Outer::Model, Outer::Deep, Outer]
true
[Outer::Model, Outer::Deep, Outer]
