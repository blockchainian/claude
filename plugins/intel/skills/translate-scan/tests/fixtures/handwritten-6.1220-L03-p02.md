\(=\) convolution of vectors \(A\) & reverse(\(B\))  
↳ inner product of all relative shifts

[FIGURE 0.226,0.123,0.872,0.194]

- useful for smoothing etc. in digital signal processing
- \(O(n^2)\) time ∴
- \(O(n^{\lg 3})\) or even \(O(n^{1+\varepsilon})\ \forall \varepsilon>0\) via Strassen-like divide & conquer tricks
- **TODAY**: \(O(n\lg n)\) time!

## Representations of polynomials:

(A) **coefficient vector** (“monomial basis”)

(B) **roots + scale**: (Fundamental Theorem of Algebra)
\[
A(x)=(x-r_0)\cdot(x-r_1)\cdot\ldots\cdot(x-r_{n-1})\cdot c
\]
- but impossible to find exact roots with \(+,-,*,\ /\)
⇒ addition hard/impossible
- multiplication: concatenate roots
- evaluate in \(O(n)\)

(C) **Samples**: \((x_0,y_0),(x_1,y_1),\ldots,(x_{n-1},y_{n-1})\)  
with \(A(x_i)=y_i\ \forall i\) & \(x_i\)’s distinct  
uniquely determine degree-\((n-1)\) polynomial \(A\)  
[Lagrange & Fundamental Theorem of Algebra]
- add/multiply each \(y_i\) (assuming \(x_i\)’s match)
- evaluate requires interpolation...
