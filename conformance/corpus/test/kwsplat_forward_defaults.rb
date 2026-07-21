# Forwarding **opts into a method with optional keyword parameters must apply
# the callee's defaults for keys absent from the forwarded hash -- a bare hash
# lookup returns nil and silently drops the default.
def target(a:, b: 99, c: "hi")
  [a, b, c]
end
def forward(**opts) = target(**opts)

p forward(a: 1)
p forward(a: 1, b: 2)
p forward(a: 1, c: "yo")
p forward(a: 1, b: 5, c: "z")

def tagged(x:, tag: :none) = [x, tag]
def ftag(**opts) = tagged(**opts)
p ftag(x: 10)
p ftag(x: 10, tag: :set)

# a literal keyword combined with a forwarded splat
def mixed(**opts) = target(a: 0, **opts)
p mixed(b: 7)
