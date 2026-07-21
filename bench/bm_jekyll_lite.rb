# Jekyll-lite: minimal static site generator. Exercises the
# frontmatter -> markdown -> html -> template pipeline that real
# static site generators use, with polymorphic patterns distinct
# from real-blog's Active Record shape (which is the dominant
# source of poly params in the current bench corpus).
#
# Patterns this exposes:
#  - Frontmatter: hash with mixed-type values (string + int)
#  - Markdown tokens: kind / text / level fields on a class
#  - Inline rendering: char-by-char walk with **bold** / *italic*
#  - Template engine: {{key}} substitution against a hash
#
# Output is deterministic so make bench can diff against CRuby.

class Frontmatter
  def initialize
    @keys = []
    @vals = []
  end

  def parse(text)
    lines = text.split("\n")
    i = 0
    while i < lines.length
      line = lines[i].strip
      if line != ""
        colon = line.index(":")
        if colon != nil && colon > 0
          key = line[0, colon].strip
          val = line[colon + 1, line.length - colon - 1].strip
          @keys.push(key)
          @vals.push(val)
        end
      end
      i = i + 1
    end
  end

  def get(key)
    i = 0
    while i < @keys.length
      if @keys[i] == key
        return @vals[i]
      end
      i = i + 1
    end
    ""
  end
end

class MdToken
  attr_reader :kind
  attr_reader :text
  attr_reader :level

  def initialize(kind, text, level)
    @kind = kind
    @text = text
    @level = level
  end
end

class MdParser
  def initialize
    @tokens = []
  end

  def parse(md)
    lines = md.split("\n")
    i = 0
    para = ""
    while i < lines.length
      line = lines[i]
      if line.length > 0 && line[0] == "#"
        if para.length > 0
          @tokens.push(MdToken.new("paragraph", para, 0))
          para = ""
        end
        level = 0
        while level < line.length && line[level] == "#"
          level = level + 1
        end
        rest = line[level, line.length - level].strip
        @tokens.push(MdToken.new("heading", rest, level))
      elsif line.strip == ""
        if para.length > 0
          @tokens.push(MdToken.new("paragraph", para, 0))
          para = ""
        end
      else
        if para.length > 0
          para = para + " "
        end
        para = para + line.strip
      end
      i = i + 1
    end
    if para.length > 0
      @tokens.push(MdToken.new("paragraph", para, 0))
    end
  end

  def tokens
    @tokens
  end
end

class Inliner
  def render(text)
    out = ""
    i = 0
    while i < text.length
      ch = text[i]
      if ch == "*" && i + 1 < text.length && text[i + 1] == "*"
        e = i + 2
        while e + 1 < text.length
          if text[e] == "*" && text[e + 1] == "*"
            break
          end
          e = e + 1
        end
        if e + 1 < text.length && text[e] == "*"
          out = out + "<strong>" + text[i + 2, e - i - 2] + "</strong>"
          i = e + 2
        else
          out = out + ch
          i = i + 1
        end
      elsif ch == "*"
        e = i + 1
        while e < text.length && text[e] != "*"
          e = e + 1
        end
        if e < text.length
          out = out + "<em>" + text[i + 1, e - i - 1] + "</em>"
          i = e + 1
        else
          out = out + ch
          i = i + 1
        end
      else
        out = out + ch
        i = i + 1
      end
    end
    out
  end
end

class HtmlEmitter
  def initialize
    @inliner = Inliner.new
  end

  def emit(tokens)
    out = ""
    i = 0
    while i < tokens.length
      t = tokens[i]
      if t.kind == "heading"
        ls = t.level.to_s
        out = out + "<h" + ls + ">" + @inliner.render(t.text) + "</h" + ls + ">\n"
      elsif t.kind == "paragraph"
        out = out + "<p>" + @inliner.render(t.text) + "</p>\n"
      end
      i = i + 1
    end
    out
  end
end

class Template
  def initialize(tmpl)
    @tmpl = tmpl
  end

  def render(vars)
    out = ""
    i = 0
    while i < @tmpl.length
      if i + 1 < @tmpl.length && @tmpl[i] == "{" && @tmpl[i + 1] == "{"
        j = i + 2
        while j + 1 < @tmpl.length && !(@tmpl[j] == "}" && @tmpl[j + 1] == "}")
          j = j + 1
        end
        key = @tmpl[i + 2, j - i - 2].strip
        if vars.has_key?(key)
          out = out + vars[key]
        end
        i = j + 2
      else
        out = out + @tmpl[i]
        i = i + 1
      end
    end
    out
  end
end

class Site
  def initialize(layout)
    @layout = layout
  end

  def build(doc)
    fm_text = ""
    body = doc
    if doc.length > 4 && doc[0, 4] == "---\n"
      end_idx = doc.index("\n---\n", 4)
      if end_idx != nil
        fm_text = doc[4, end_idx - 4]
        body = doc[end_idx + 5, doc.length - end_idx - 5]
      end
    end
    fm = Frontmatter.new
    fm.parse(fm_text)
    parser = MdParser.new
    parser.parse(body)
    emitter = HtmlEmitter.new
    content = emitter.emit(parser.tokens)
    tmpl = Template.new(@layout)
    vars = {
      "title" => fm.get("title"),
      "author" => fm.get("author"),
      "content" => content
    }
    tmpl.render(vars)
  end
end

LAYOUT = "<html><head><title>{{title}}</title></head><body><h1>{{title}}</h1><p class='by'>by {{author}}</p>\n{{content}}</body></html>\n"

site = Site.new(LAYOUT)

DOC1 = "---\ntitle: Hello World\nauthor: Matz\n---\n# Welcome\n\nThis is a **bold** intro paragraph.\n\n## Why static\n\nFast, *cacheable*, and **portable**.\n"

DOC2 = "---\ntitle: Second Post\nauthor: spinel\n---\n# Second\n\nA shorter article with *one* emphasis.\n"

puts site.build(DOC1)
puts "---"
puts site.build(DOC2)
