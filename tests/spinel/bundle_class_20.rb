# Bundled tests:
#   - issue207_factory_pattern
#   - issue207_full_repro
#   - issue208_inherited_class_method

# === issue207_factory_pattern ===
# Issue #207: factory class method (`def self.from_raw`) using
# implicit `new`, calling a setter on the new instance whose value
# came from a poly-hash-returning fetch. Several composing gaps:
#
#   1. Bare `new` inside a `def self.<m>` body must resolve to
#      <CurrentClass>.new (returning obj_<CurrentClass>).
#   2. The class method's parameter type must widen from the
#      class-constant call site (`T_issue207_factory_pattern_T.from_raw(p)` → params: obj_P).
#   3. The setter's poly arg must widen the receiving ivar's slot
#      type (sp_RbVal field, not mrb_int).
#   4. The fetch's nil-default param must widen from the call-site
#      string default ("" → string?).

class T_issue207_factory_pattern_P
  def initialize(h)
    @h = h
  end
  def fetch(key, default = nil)
    return @h[key] if @h.key?(key)
    default
  end
end

class T_issue207_factory_pattern_T
  attr_accessor :title
  def self.from_raw(params)
    instance = new
    instance.title = params.fetch(:title, "")
    instance
  end
end

p = T_issue207_factory_pattern_P.new({title: "hello", count: 42})
t = T_issue207_factory_pattern_T.from_raw(p)
puts t.title

# === issue207_full_repro ===
# Issue #207: full Sam Ruby repro — Rails-style ActiveRecord
# T_issue207_full_repro_Parameters wrapper with symbolize_keys recursion plus a typed
# Params factory using fetch.
#
# This combines several fixes that had to land together:
#
# - Implicit `new` inside `def self.<m>` resolves to the enclosing
#   class's constructor (#207 partial fix).
# - cls method body return inference picks up locals (#207 partial
#   fix).
# - attr_writer call inside cls method body widens the ivar slot.
# - Class-constant call sites (T.from_raw(p)) widen the cls method's
#   parameter types.
# - Static is_a? / kind_of? on a known-concrete-type receiver
#   eliminates the unreachable arm so the dead recursion call
#   doesn't land in C and trip the type checker.

class T_issue207_full_repro_Parameters
  def initialize(hash = {})
    @hash = symbolize_keys(hash)
  end

  def symbolize_keys(input)
    out = {}
    input.each do |k, v|
      sym = k.is_a?(Symbol) ? k : k.to_s.to_sym
      out[sym] = v.is_a?(Hash) ? symbolize_keys(v) : v
    end
    out
  end

  def fetch(key, default = nil)
    sym = key.to_sym
    return @hash[sym] if @hash.key?(sym)
    default
  end
end

class T_issue207_full_repro_ArticleParams
  def title; @title; end
  def title=(value); @title = value; end

  def self.from_raw(params)
    instance = new
    instance.title = params.fetch(:title, "")
    instance
  end
end

p = T_issue207_full_repro_Parameters.new({title: "hello"})
ap = T_issue207_full_repro_ArticleParams.from_raw(p)
puts ap.title

# === issue208_inherited_class_method ===
# Issue #208: a class method (`def self.<name>`) defined on the
# parent class must dispatch when called via the subclass. Spinel
# previously emitted "cannot resolve call to '<method>' on
# class_<Subclass>" and substituted 0 because cls_cmethod lookup
# only walked the immediate class.

class T_issue208_inherited_class_method_Base
  def self.all
    [42, 1, 7]
  end
end

# Single level
class T_issue208_inherited_class_method_Leaf < T_issue208_inherited_class_method_Base; end
puts T_issue208_inherited_class_method_Leaf.all.size                   # 3

# Multi level
class T_issue208_inherited_class_method_Mid < T_issue208_inherited_class_method_Base; end
class T_issue208_inherited_class_method_Deep < T_issue208_inherited_class_method_Mid; end
puts T_issue208_inherited_class_method_Mid.all.size                    # 3
puts T_issue208_inherited_class_method_Deep.all.size                   # 3

# Direct call on the defining class still works
puts T_issue208_inherited_class_method_Base.all.size                   # 3

