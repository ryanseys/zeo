module M
  def self.helper?
    true
  end
  if helper? && RUBY_VERSION[0, 3] == '1.9'
    def self.append_features(mod)
      raise "1.9-only path ran"
    end
  end
end

class K
  include M
end
p K.ancestors.include?(M)
