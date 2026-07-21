# AO render benchmark
# Original program (C) Syoyo Fujita in JavaScript (and other languages)
#      https://code.google.com/p/aobench/
# Ruby(yarv2llvm) version by Hideki Miura
# mruby version by Hideki Miura
#

IMAGE_WIDTH = Integer(ARGV[0] || 64)
IMAGE_HEIGHT = IMAGE_WIDTH
NSUBSAMPLES = 2
NAO_SAMPLES = 8

class Rand
  def initialize
    @x = 123456789
    @y = 362436069
    @z = 521288629
    @w = 88675123
  end
  def rand
    x = @x
    t = x ^ ((x & 0xfffff) << 11)
    w = @w
    @x, @y, @z = @y, @z, w
    w = @w = (w ^ (w >> 19) ^ (t ^ (t >> 8)))
    r = (w % 536870912).to_f / 536870912.0
    r
  end
end
RAND = Rand.new

class Vec
  def initialize(x, y, z)
    @x = x
    @y = y
    @z = z
  end

  def x=(v); @x = v; end
  def y=(v); @y = v; end
  def z=(v); @z = v; end
  def x; @x; end
  def y; @y; end
  def z; @z; end

  def vadd(b)
    Vec.new(@x + b.x, @y + b.y, @z + b.z)
  end

  def vsub(b)
    Vec.new(@x - b.x, @y - b.y, @z - b.z)
  end

  def vcross(b)
    Vec.new(@y * b.z - @z * b.y,
            @z * b.x - @x * b.z,
            @x * b.y - @y * b.x)
  end

  def vdot(b)
    r = @x * b.x + @y * b.y + @z * b.z
    r
  end

  def vlength
    Math.sqrt(@x * @x + @y * @y + @z * @z)
  end

  def vnormalize
    len = vlength
    v = Vec.new(@x, @y, @z)
    if len > 1.0e-17
      v.x = v.x / len
      v.y = v.y / len
      v.z = v.z / len
    end
    v
  end
end


class Sphere
  def initialize(center, radius)
    @center = center
    @radius = radius
  end

  def center; @center; end
  def radius; @radius; end

  def intersect(ray, isect)
    rs = ray.org.vsub(@center)
    b = rs.vdot(ray.dir)
    c = rs.vdot(rs) - (@radius * @radius)
    d = b * b - c
    if d > 0.0
      t = - b - Math.sqrt(d)

      if t > 0.0 and t < isect.t
        isect.t = t
        isect.hit = true
        isect.pl = Vec.new(ray.org.x + ray.dir.x * t,
                          ray.org.y + ray.dir.y * t,
                          ray.org.z + ray.dir.z * t)
        n = isect.pl.vsub(@center)
        isect.n = n.vnormalize
      end
    end
  end
end

class Plane
  def initialize(p, n)
    @p = p
    @n = n
  end

  def intersect(ray, isect)
    d = -@p.vdot(@n)
    v = ray.dir.vdot(@n)
    v0 = v
    if v < 0.0
      v0 = -v
    end
    if v0 < 1.0e-17
      return
    end

    t = -(ray.org.vdot(@n) + d) / v

    if t > 0.0 and t < isect.t
      isect.hit = true
      isect.t = t
      isect.n = @n
      isect.pl = Vec.new(ray.org.x + t * ray.dir.x,
                        ray.org.y + t * ray.dir.y,
                        ray.org.z + t * ray.dir.z)
    end
  end
end

class Ray
  def initialize(org, dir)
    @org = org
    @dir = dir
  end

  def org; @org; end
  def org=(v); @org = v; end
  def dir; @dir; end
  def dir=(v); @dir = v; end
end

class Isect
  def initialize
    @t = 10000000.0
    @hit = false
    @pl = Vec.new(0.0, 0.0, 0.0)
    @n = Vec.new(0.0, 0.0, 0.0)
  end

  def t; @t; end
  def t=(v); @t = v; end
  def hit; @hit; end
  def hit=(v); @hit = v; end
  def pl; @pl; end
  def pl=(v); @pl = v; end
  def n; @n; end
  def n=(v); @n = v; end
end

def clamp(f)
  i = f * 255.5
  if i > 255.0
    i = 255.0
  end
  if i < 0.0
    i = 0.0
  end
  i.to_i
end

class Basis
  def initialize
    @b0 = Vec.new(0.0, 0.0, 0.0)
    @b1 = Vec.new(0.0, 0.0, 0.0)
    @b2 = Vec.new(0.0, 0.0, 0.0)
  end
  def b0; @b0; end
  def b0=(v); @b0 = v; end
  def b1; @b1; end
  def b1=(v); @b1 = v; end
  def b2; @b2; end
  def b2=(v); @b2 = v; end

  def compute(n)
    @b2 = Vec.new(n.x, n.y, n.z)
    @b1 = Vec.new(0.0, 0.0, 0.0)

    if n.x < 0.6 and n.x > -0.6
      @b1.x = 1.0
    elsif n.y < 0.6 and n.y > -0.6
      @b1.y = 1.0
    elsif n.z < 0.6 and n.z > -0.6
      @b1.z = 1.0
    else
      @b1.x = 1.0
    end

    @b0 = @b1.vcross(@b2)
    @b0 = @b0.vnormalize

    @b1 = @b2.vcross(@b0)
    @b1 = @b1.vnormalize
  end
end

class Scene
  def initialize
    @s0 = Sphere.new(Vec.new(-2.0, 0.0, -3.5), 0.5)
    @s1 = Sphere.new(Vec.new(-0.5, 0.0, -3.0), 0.5)
    @s2 = Sphere.new(Vec.new(1.0, 0.0, -2.2), 0.5)
    @plane = Plane.new(Vec.new(0.0, -0.5, 0.0), Vec.new(0.0, 1.0, 0.0))
  end

  def ambient_occlusion(isect)
    basis = Basis.new
    basis.compute(isect.n)

    ntheta    = NAO_SAMPLES
    nphi      = NAO_SAMPLES
    eps       = 0.0001
    occlusion = 0.0

    p0 = Vec.new(isect.pl.x + eps * isect.n.x,
                isect.pl.y + eps * isect.n.y,
                isect.pl.z + eps * isect.n.z)
    nphi.times do
      ntheta.times do
        r = RAND.rand
        phi = 2.0 * 3.14159265 * RAND.rand
        x = Math.cos(phi) * Math.sqrt(1.0 - r)
        y = Math.sin(phi) * Math.sqrt(1.0 - r)
        z = Math.sqrt(r)

        rx = x * basis.b0.x + y * basis.b1.x + z * basis.b2.x
        ry = x * basis.b0.y + y * basis.b1.y + z * basis.b2.y
        rz = x * basis.b0.z + y * basis.b1.z + z * basis.b2.z

        raydir = Vec.new(rx, ry, rz)
        ray = Ray.new(p0, raydir)

        occisect = Isect.new
        @s0.intersect(ray, occisect)
        @s1.intersect(ray, occisect)
        @s2.intersect(ray, occisect)
        @plane.intersect(ray, occisect)
        if occisect.hit
          occlusion = occlusion + 1.0
        end
      end
    end

    occlusion = (ntheta.to_f * nphi.to_f - occlusion) / (ntheta.to_f * nphi.to_f)
    Vec.new(occlusion, occlusion, occlusion)
  end

  def render(w, h, nsubsamples)
    nsf = nsubsamples.to_f
    nsfs = nsf * nsf
    h.times do |y|
      w.times do |x|
        rad = Vec.new(0.0, 0.0, 0.0)

        # Subsampling
        nsubsamples.times do |v|
          nsubsamples.times do |u|
            wf = w.to_f
            hf = h.to_f
            xf = x.to_f
            yf = y.to_f
            uf = u.to_f
            vf = v.to_f

            px = (xf + (uf / nsf) - (wf / 2.0)) / (wf / 2.0)
            py = -(yf + (vf / nsf) - (hf / 2.0)) / (hf / 2.0)

            eye = Vec.new(px, py, -1.0).vnormalize

            ray = Ray.new(Vec.new(0.0, 0.0, 0.0), eye)

            isect = Isect.new
            @s0.intersect(ray, isect)
            @s1.intersect(ray, isect)
            @s2.intersect(ray, isect)
            @plane.intersect(ray, isect)
            if isect.hit
              col = ambient_occlusion(isect)
              rad.x = rad.x + col.x
              rad.y = rad.y + col.y
              rad.z = rad.z + col.z
            end
          end
        end

        r = rad.x / nsfs
        g = rad.y / nsfs
        b = rad.z / nsfs
        print clamp(r).chr
        print clamp(g).chr
        print clamp(b).chr
      end
    end
  end
end

# File.open("ao.ppm", "w") do |fp|
  printf("P6\n")
  printf("%d %d\n", IMAGE_WIDTH, IMAGE_HEIGHT)
  printf("255\n")
  Scene.new.render(IMAGE_WIDTH, IMAGE_HEIGHT, NSUBSAMPLES)
#  Scene.new.render(256, 256, 2)
# end
