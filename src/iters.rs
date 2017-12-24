// This file is part of faster, the SIMD library for humans.
// Copyright 2017 Adam Niederer <adam.niederer@gmail.com>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use vecs::{Packable, Packed};

/// An iterator which automatically packs the values it iterates over into SIMD
/// vectors.
pub trait PackedIterator : Sized + ExactSizeIterator {
    type Scalar : Packable;
    type Vector : Packed<Scalar = Self::Scalar>;

    #[inline(always)]
    fn width(&self) -> usize {
        Self::Vector::WIDTH
    }

    /// Return the length of this iterator, measured in scalar elements.
    fn scalar_len(&self) -> usize;

    /// Return the current position of this iterator, measured in scalar
    /// elements.
    fn scalar_position(&self) -> usize;

    /// Pack and return a vector containing the next `self.width()` elements
    /// of the iterator, or return None if there aren't enough elements left
    fn next_vector(&mut self) -> Option<Self::Vector>;

    /// Pack and return a partially full vector containing upto the next
    /// `self.width()` of the iterator, or None if no elements are left.
    /// Elements which are not filled are instead initialized to default.
    fn next_partial(&mut self, default: Self::Vector) -> Option<Self::Vector>;

    #[inline(always)]
    /// Return an iterator which calls `func` on vectors of elements.
    fn simd_map<A, B, F>(self, func: F) -> PackedMap<Self, F>
        where F : FnMut(Self::Vector) -> A, A : Packed<Scalar = B>, B : Packable {
        PackedMap {
            iter: self,
            func: func
        }
    }

    #[inline(always)]
    /// Return a vector generated by reducing `func` over accumulator `start`
    /// and the values of this iterator, initializing all vectors to `default`
    /// before populating them with elements of the iterator.
    ///
    /// # Examples
    ///
    /// ```
    /// extern crate faster;
    /// use faster::*;
    ///
    /// # fn main() {
    /// let reduced = (&[2.0f32; 100][..]).simd_iter()
    ///    .simd_reduce(f32s::splat(0.0), f32s::splat(0.0), |acc, v| *acc + *v);
    /// # }
    /// ```
    ///
    /// In this example, on a machine with 4-element vectors, the argument to
    /// the last call of the closure is
    ///
    /// ```rust,ignore
    /// [ 2.0 | 2.0 | 2.0 | 2.0 ]
    /// ```
    ///
    /// and the result of the reduction is
    ///
    /// ```rust,ignore
    /// [ 50.0 | 50.0 | 50.0 | 50.0 ]
    /// ```
    ///
    /// whereas on a machine with 8-element vectors, the last call is passed
    ///
    /// ```rust,ignore
    /// [ 2.0 | 2.0 | 2.0 | 2.0 | 0.0 | 0.0 | 0.0 | 0.0 ]
    /// ```
    ///
    /// and the result of the reduction is
    ///
    /// ```rust,ignore
    /// [ 26.0 | 26.0 | 26.0 | 26.0 | 24.0 | 24.0 | 24.0 | 24.0 ]
    /// ```
    ///
    /// # Footgun Warning
    ///
    /// The results of `simd_reduce` are not portable, and it is your
    /// responsibility to interepret the result in such a way that the it is
    /// consistent across different architectures. See [`Packed::sum`] and
    /// [`Packed::product`] for built-in functions which may be helpful.
    ///
    /// [`Packed::sum`]: vecs/trait.Packed.html#tymethod.sum
    /// [`Packed::product`]: vecs/trait.Packed.html#tymethod.product
    fn simd_reduce<A, F>(&mut self, start: A, default: Self::Vector, mut func: F) -> A
        where F : FnMut(A, Self::Vector) -> A {
        let mut acc: A;
        if let Some(v) = self.next_vector() {
            acc = func(start, v);
            while let Some(v) = self.next_vector() {
                acc = func(acc, v);
            }
            if let Some(v) = self.next_partial(default) {
                acc = func(acc, v);
            }
            debug_assert!(self.next_partial(default).is_none());
            acc
        } else if let Some(v) = self.next_partial(default) {
            acc = func(start, v);
            while let Some(v) = self.next_partial(default) {
                acc = func(acc, v);
            }
            debug_assert!(self.next_partial(default).is_none());
            acc
        } else {
            start
        }
    }
}

#[derive(Debug)]
pub struct PackedIter<'a, T : 'a + Packable> {
    pub position: usize,
    pub data: &'a [T],
}

#[derive(Debug)]
pub struct PackedMap<I, F> {
    pub iter: I,
    pub func: F,
}

impl<'a, T> Iterator for PackedIter<'a, T> where T : Packable {
    type Item = <PackedIter<'a, T> as PackedIterator>::Scalar;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        self.data.get(self.position).map(|v| { self.position += 1; *v })
    }

    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.data.len() - self.position;
        (remaining, Some(remaining))
    }
}

impl<'a, T> ExactSizeIterator for PackedIter<'a, T>
    where T : Packable {

    #[inline(always)]
    fn len(&self) -> usize {
        self.data.len()
    }
}

impl<'a, T> PackedIterator for PackedIter<'a, T> where T : Packable {
    type Vector = <T as Packable>::Vector;
    type Scalar = T;

    #[inline(always)]
    fn scalar_len(&self) -> usize {
        self.data.len()
    }

    #[inline(always)]
    fn scalar_position(&self) -> usize {
        self.position
    }

    #[inline(always)]
    fn next_vector(&mut self) -> Option<Self::Vector> {
        if self.position + self.width() <= self.scalar_len() {
            let ret = Some(Self::Vector::load(self.data, self.position));
            self.position += Self::Vector::WIDTH;
            ret
        } else {
            None
        }
    }

    #[inline(always)]
    fn next_partial(&mut self, default: Self::Vector) -> Option<Self::Vector> where T : Packable {
        if self.position < self.scalar_len() {
            let mut ret = Self::Vector::splat(default.extract(0));
            for i in 0..self.scalar_len() - self.position {
                ret = ret.replace(i, self.data[self.position + i].clone());
            }

            self.position = self.scalar_len();
            Some(ret)
        } else {
            None
        }
    }
}

impl<T: PackedIterator> IntoPackedIterator for T {
    type Iter = T;

    #[inline(always)]
    fn into_simd_iter(self) -> T {
        self
    }
}

pub trait IntoPackedIterator {
    type Iter: PackedIterator;

    /// Return an iterator over this data which will automatically pack
    /// values into SIMD vectors. See `PackedIterator::simd_map` and
    /// `PackedIterator::simd_reduce` for more information.
    fn into_simd_iter(self) -> Self::Iter;
}

pub trait IntoPackedRefIterator<'a> {
    type Iter: PackedIterator;

    /// Return an iterator over this data which will automatically pack
    /// values into SIMD vectors. See `PackedIterator::simd_map` and
    /// `PackedIterator::simd_reduce` for more information.
    fn simd_iter(&'a self) -> Self::Iter;
}

pub trait IntoPackedRefMutIterator<'a> {
    type Iter: PackedIterator;

    /// Return an iterator over this data which will automatically pack
    /// values into SIMD vectors. See `PackedIterator::simd_map` and
    /// `PackedIterator::simd_reduce` for more information.
    fn simd_iter_mut(&'a mut self) -> Self::Iter;
}

// Impl ref & ref mut iterators for moved iterator
impl<'a, I: 'a + ?Sized> IntoPackedRefIterator<'a> for I
    where &'a I: IntoPackedIterator {
    type Iter = <&'a I as IntoPackedIterator>::Iter;

    fn simd_iter(&'a self) -> Self::Iter {
        self.into_simd_iter()
    }
}

impl<'a, I: 'a + ?Sized> IntoPackedRefMutIterator<'a> for I
    where &'a mut I: IntoPackedIterator {
    type Iter = <&'a mut I as IntoPackedIterator>::Iter;

    fn simd_iter_mut(&'a mut self) -> Self::Iter {
        self.into_simd_iter()
    }
}

impl<A, B, I, F> Iterator for PackedMap<I, F>
    where I : PackedIterator<Scalar = <I as Iterator>::Item>, <I as Iterator>::Item : Packable, F : FnMut(I::Vector) -> A, A : Packed<Scalar = B>, B : Packable {
    type Item = B;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        Some((&mut self.func)(I::Vector::splat(self.iter.next()?)).coalesce())
    }

    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self.len() - self.iter.scalar_position() * self.width()) / self.width();
        (remaining, Some(remaining))
    }
}

impl<'a, I, F> ExactSizeIterator for PackedMap<I, F>
    where Self : PackedIterator, I : PackedIterator {
    #[inline(always)]
    fn len(&self) -> usize {
        self.iter.len()
    }
}

impl<'a, A, B, I, F> PackedIterator for PackedMap<I, F>
    where I : PackedIterator<Scalar = <I as Iterator>::Item>, <I as Iterator>::Item : Packable, F : FnMut(I::Vector) -> A, A : Packed<Scalar = B>, B : Packable {
    type Vector = A;
    type Scalar = B;


    #[inline(always)]
    fn scalar_len(&self) -> usize {
        self.iter.scalar_len()
    }

    #[inline(always)]
    fn scalar_position(&self) -> usize {
        self.iter.scalar_position()
    }

    #[inline(always)]
    fn next_vector(&mut self) -> Option<Self::Vector> {
        self.iter.next_vector().map(&mut self.func)
    }

    #[inline(always)]
    fn next_partial(&mut self, default: Self::Vector) -> Option<Self::Vector> {
        // TODO: Take a user-defined default and return number of elements actually mapped
        self.iter.next_partial(A::default()).map(&mut self.func)
    }
}

pub trait IntoScalar<T> where T : Packable {
    type Scalar : Packable;
    type Vector : Packed<Scalar = Self::Scalar>;

    /// Take an iterator of SIMD vectors, store them in-order in a Vec, and
    /// return the vec.
    #[cfg(not(feature = "no-std"))]
    fn scalar_collect(&mut self) -> Vec<T>;

    /// Take an iterator of SIMD vectors and store them in-order in `fill`.
    fn scalar_fill<'a>(&mut self, fill: &'a mut [T]) -> &'a mut [T];
}

impl<'a, T, I> IntoScalar<T> for I
    where I : PackedIterator<Scalar = T>, I::Vector : Packed<Scalar = T>, T : Packable {
    type Scalar = I::Scalar;
    type Vector = I::Vector;

    #[inline(always)]
    #[cfg(not(feature = "no-std"))]
    fn scalar_collect(&mut self) -> Vec<Self::Scalar> {
        let mut offset = 0;
        let mut ret = Vec::with_capacity(self.len());

        unsafe {
            ret.set_len(self.len());
            while let Some(vec) = self.next_vector() {
                vec.store(ret.as_mut_slice(), offset);
                offset += Self::Vector::WIDTH;
            }
            while let Some(scl) = self.next() {
                ret[offset] = scl;
                offset += 1;
            }
        }
        ret
    }

    #[inline(always)]
    fn scalar_fill<'b>(&mut self, fill: &'b mut [Self::Scalar]) -> &'b mut [Self::Scalar] {
        let mut offset = 0;

        while let Some(vec) = self.next_vector() {
            vec.store(fill, offset);
            offset += Self::Vector::WIDTH;
        }

        while let Some(scl) = self.next() {
            fill[offset] = scl;
            offset += 1;
        }
        fill
    }
}
