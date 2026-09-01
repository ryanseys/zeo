# The `class` KEYWORD names a class and binds its constant BEFORE the
# superclass's `inherited` hook fires -- CRuby's rb_define_class_id_under
# order -- in an eval snippet too. Only `Class.new` shows the hook a nil
# name. rss's Element.inherited reads `klass.name` for the tag name.
module M
  class Base
    def self.inherited(k)
      super
      p [:inherited, k.name, M.const_defined?(:Kid, false)]
    end
  end
  M.module_eval("class Kid < Base; end")
  p M::Kid.name
end
anon = Class.new(M::Base)
p anon.name
