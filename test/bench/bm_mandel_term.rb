def mandelbrot(c_r, c_i)
  limit=95
  iterations=0
  cr = (c_r * 100).to_i
  ci = (c_i * 100).to_i
  zr = zi = 0
  # Avoid sqrt by squaring the threshold: sqrt(x) < 1000 => x < 1000000
  while iterations<limit
    zr2 = zr*zr
    zi2 = zi*zi
    break if zr2+zi2 >= 1000000
    zr, zi = (zr2-zi2)/100+cr, (zr*zi*2)/100+ci
    iterations+=1
  end
  return iterations
end

def mandel_calc(min_r, min_i, max_r, max_i, res)
  cur_i = min_i
  while cur_i > max_i
    putc "|"
    cur_r = min_r
    while cur_r < max_r
      ch = 127 - mandelbrot(cur_r, cur_i)
      putc ch  # Use putc with integer - no string allocation!
      cur_r += res
    end
    putc "|"
    putc "\n"
    cur_i -= res
  end
end

mandel_calc(-2, 1, 1, -1, 0.04)
__END__
|||{{{{{{{{{{{{{{{{{{{{{{{{zzzzzzzzzzzzyyyyxxwurstq xyyyzzzzzzz{{{{{{{{{{{{{|
||{{{{{{{{{{{{{{{{{{{{{{{zzzzzzzzzzzzyyyyyxxwwvsnltvxxyyyzzzzzzz{{{{{{{{{{{{|
|{{{{{{{{{{{{{{{{{{{{{{zzzzzzzzzzzzzyyyyyxxxwvurhruvwxyyyyzzzzzzz{{{{{{{{{{{|
|{{{{{{{{{{{{{{{{{{{{zzzzzzzzzzzzzyyyyyyxxwvtorY [stuwxyyyyzzzzzzz{{{{{{{{{{|
|{{{{{{{{{{{{{{{{{{zzzzzzzzzzzzzyyyyyyxxxxwuqcb   oStwxxyyyyyzzzzzz{{{{{{{{{|
|{{{{{{{{{{{{{{{{zzzzzzzzzzzzzzyyyyyxxxxwwwuri     atvwxxxyyyyzzzzz{{{{{{{{{|
|{{{{{{{{{{{{{{{zzzzzzzzzzzzzyyyyyyxxwwwwwvup      gtvwwxxxxyyyzzzzz{{{{{{{{|
|{{{{{{{{{{{{{zzzzzzzzzzzzzyyyyyyxxwvvuvvuutrj     [suuvwwwwwxyyzzzzz{{{{{{{|
|{{{{{{{{{{{{zzzzzzzzzzzzyyyyyyyxxwtrpqttmoool:   jnprprvvvvpwxyzzzzz{{{{{{{|
|{{{{{{{{{{{zzzzzzzzzzzyyyyyyyyxxxwuh]mirl _         / gttstmuxyyzzzz{{{{{{{|
|{{{{{{{{{zzzzzzzzzzyyyyyyyyyyxxxwwuqf  kP             $oqagfswyyzzzzz{{{{{{|
|{{{{{{{{zzzzzzzzzyyyyyyyyyyxxxxwwvuoa                   *  kswxyyzzzz{{{{{{|
|{{{{{{{zzzzzzzzyyyyyyyyyxxxxxxwwvusqm                      gvwxyyzzzzz{{{{{|
|{{{{{{zzzzzzzzyyyyxxxxxxxxxxwwwvq kk                      ntvwxyyzzzzz{{{{{|
|{{{{{zzzzzzzzyyyxwvwwxxwwwwwwwvvtf                         Juvxyyzzzzz{{{{{|
|{{{{zzzzzzzzyyyyxwtuvvvvgvvvvvvusoR                        nrrvxyyzzzz{{{{{|
|{{{zzzzzzzzyyyyxxvosututPtuuuuutN                            ^uxyyzzzz{{{{{|
|{{zzzzzzzzyyyyxxxwt_Qjrsn[rVtttsW                            swxyyzzzzz{{{{|
|{zzzzzzzyyyyyxxxwvuso  ai j=Rrsqg                           kuwxyyzzzzz{{{{|
|zzzzzzyyyyyyxxxwwvurj       Y,qp                             gwxyyzzzzz{{{{|
|zzzzzyyyyyyxxxwvvuq^e         ml                            ouxxyyzzzzz{{{{|
|zzzyyyyyyxxxxwtuutrg          Qg                            kvxyyyzzzzz{{{{|
|zzyyyyxxxxxwwvtmrqo            b                            twxyyyzzzzz{{{{|
|zyyxxwwwwwwwvutikLi                                        [vwxyyyzzzzz{{{{|
|yxxwwusvvruutrpl                                          quwxxyyyzzzzz{{{{|
|                                                         lsuwxxyyyzzzzz{{{{|
|yxxxwuuvvtuutsol                                           tvxxyyyzzzzz{{{{|
|zyyyxxwwwwwwvuslmlj            W                           mvwxyyyzzzzz{{{{|
|zzyyyyxxxxxwwvtrsrlX           c                            rwxyyyzzzzz{{{{|
|zzzzyyyyyxxxxwsuuto?          Kj                            pvxyyyzzzzz{{{{|
|zzzzzyyyyyyxxxwwvuqqh@        pm                            Jqwxyyzzzzz{{{{|
|zzzzzzzyyyyyxxxwwvup   `    [prp                            lpwxyyzzzzz{{{{|
|{zzzzzzzyyyyyxxxwvusl^nn<\nprssr                            puwxyyzzzzz{{{{|
|{{zzzzzzzzyyyyxxwvskpptrbqsruutso                             wxyyzzzz{{{{{|
|{{{zzzzzzzzyyyyxxvotuuuujuvvvvutpe                         eH_wxyyzzzz{{{{{|
|{{{{zzzzzzzzyyyyxwrvwwwwuvwwwvvuso]                        ottwyyzzzzz{{{{{|
|{{{{{zzzzzzzzyyyxwwwxxxxwwwwwwvvt] [                       rvwxyyzzzzz{{{{{|
|{{{{{{zzzzzzzzyyyyxxxxxxxxxxxwwwtFogY                    [3svwxyyzzzzz{{{{{|
|{{{{{{{zzzzzzzzyyyyyyyyyxxxxxxxwvutrm                   W  Jvxxyyzzzz{{{{{{|
|{{{{{{{{zzzzzzzzzyyyyyyyyyyxxxxxwvurM                  :=  mcxyyzzzzz{{{{{{|
|{{{{{{{{{zzzzzzzzzzyyyyyyyyyyxxxwwtp6 bnh             nrr]pdpxyyzzzzz{{{{{{|
|{{{{{{{{{{{zzzzzzzzzzzyyyyyyyyxxxwunhpqseEca      \5m]Huuut vxyyzzzz{{{{{{{|
|{{{{{{{{{{{{zzzzzzzzzzzzzyyyyyyyxwntspuuspsqn    hprtsuvwwvpwyyzzzzz{{{{{{{|
|{{{{{{{{{{{{{zzzzzzzzzzzzzzyyyyyyxwwvvvvvutr      otuvwwxxxxxyzzzzz{{{{{{{{|
|{{{{{{{{{{{{{{{zzzzzzzzzzzzzyyyyyyxxxwwwwvurV     juvwxxxxyyyyzzzzz{{{{{{{{|
|{{{{{{{{{{{{{{{{{zzzzzzzzzzzzzyyyyyyxxxxwwusg     ktwxxxyyyyyzzzzz{{{{{{{{{|
|{{{{{{{{{{{{{{{{{{zzzzzzzzzzzzzzyyyyyyxxxwv^k E jqguwxxyyyyzzzzzz{{{{{{{{{{|
|{{{{{{{{{{{{{{{{{{{{zzzzzzzzzzzzzyyyyyyxxxwusrj etuwxxyyyyzzzzzzz{{{{{{{{{{|
|{{{{{{{{{{{{{{{{{{{{{{zzzzzzzzzzzzzyyyyyxxxwvtqesuvwxyyyyzzzzzzz{{{{{{{{{{{|
||{{{{{{{{{{{{{{{{{{{{{{{zzzzzzzzzzzzyyyyyxxwwusgpuvxyyyyzzzzzzz{{{{{{{{{{{{|
