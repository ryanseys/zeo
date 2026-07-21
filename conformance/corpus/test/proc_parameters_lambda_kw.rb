# Proc#parameters(lambda:) forces the reporting view (#2693): true reports
# plain positionals as :req, false as :opt, nil follows the receiver's own
# nature. Defaulted positionals stay :opt in every view, and rest/key/block
# kinds never change. Kinds are stored canonically (lambda-style) and remapped
# at print time, which also fixes numbered params on a lambda (:req, not :opt).
p proc { |x, y| }.parameters
p proc { |x, y| }.parameters(lambda: true)
p ->(x, y) {}.parameters
p ->(x, y) {}.parameters(lambda: false)
p proc { |x, y=1| }.parameters(lambda: true)
p ->(x, y=1) {}.parameters(lambda: false)
p proc { |a, *r, k: 1, &b| }.parameters(lambda: true)
p proc { |x| }.parameters(lambda: nil)
pr = proc { |x, y| }
p pr.parameters(lambda: true)
p lambda { _1 }.parameters
p proc { _1 + _2 }.parameters(lambda: true)
