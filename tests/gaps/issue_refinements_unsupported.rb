# Refinements (Module#refine / Kernel#using) are entirely unimplemented --
# zeo raises NoMethodError for `refine` instead of scoping the refined
# methods to the `using` call site.
module RefMod2
  refine String do
    def shout
      upcase + "!"
    end
  end
end
using RefMod2
p "hi".shout
