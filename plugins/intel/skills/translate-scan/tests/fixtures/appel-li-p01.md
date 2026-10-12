# Virtual Memory Primitives for User Programs

Andrew W. Appel and Kai Li

Department of Computer Science  
Princeton University

## Abstract

Memory Management Units (MMUs) are traditionally used by operating systems to implement disk-paged virtual memory. Some operating systems allow user programs to specify the protection level (inaccessible, read-only, read-write) of pages, and allow user programs to handle protection violations, but these mechanisms are not always robust, efficient, or well-matched to the needs of applications.

We survey several user-level algorithms that make use of page-protection techniques, and analyze their common characteristics, in an attempt to answer the question, “What virtual-memory primitives should the operating system provide to user processes, and how well do today’s operating systems provide them?”

## 1 Introduction

The “traditional” purpose of virtual memory is to increase the size of the address space visible to user programs, by allowing only the frequently-accessed subset of the address space to be resident in physical memory. But virtual memory has been used for many other purposes. Operating systems can share pages between processes, make instruction-spaces read-only (and thus guaranteed re-entrant), make portions of memory zeroed-on-demand or copy-on-write, and so on [18]. In fact, there is a large class of “tricks” that operating systems can perform using the page protection hardware.

Modern operating systems allow user programs to perform such tricks too, by allowing user programs to provide “handlers” for protection violations. Unix, for example, allows a user process to specify that a particular subroutine is to be executed whenever a segmentation-fault signal is generated. When a program accesses memory beyond its legal virtual address range, a user-friendly error message can be produced by the user-provided signal handler, instead of the ominous “segmentation fault: core dumped.”

Note: Permission to copy without fee all or part of this material is granted provided that the copies are not made or distributed for direct commercial advantage, the ACM copyright notice and the title of the publication and its date appear, and notice is given that copying is by permission of the Association for Computing Machinery. To copy otherwise, or to republish, requires a fee and/or specific permission.  
© 1991 ACM 0-89791-380-9/91/0003-0096...$1.50

This simple example of a user-mode fault handler is “dangerous,” because it may lead the operating-system and hardware designers to believe that user-mode fault-handlers need not be entered efficiently (which is certainly the case for the “graceful error shutdown” example). But there are much more interesting applications of user-mode fault handlers. These applications exercise the page-protection and fault-handling mechanisms quite strenuously, and should be understood by operating-system implementors.

This paper describes several algorithms that make use of page-protection techniques. In many cases, the algorithms can substitute the use of “conventional” paging hardware for the “special” microcode that has sometimes been used. On shared-memory multiprocessors, the algorithms use page-protection hardware to achieve medium-grained synchronization with low overhead, in order to avoid synchronization instruction sequences that have notable overhead.

We have benchmarked a number of systems to analyze how well today’s operating systems support user-level page-protection techniques. Finally, from these algorithms we draw lessons about page-protection costs, the utility of memory-mapping mechanisms, translation-buffer shootdowns, page sizes and other aspects of operating system implementation.

## 2 Virtual memory primitives

Each of the algorithms we will describe require some of the following virtual-memory services from the operating system:

TRAP: handle page-fault traps in user mode;  
PROT1: decrease the accessibility of a page;  
PROTN: decrease the accessibility of N pages;  
UNPROT: increase the accessibility of a page;  
DIRTY: return a list of dirtied pages since the MAP2: previous call.  
MAP2: map the same physical page at two different virtual addresses, at different levels of protection, in the same address space.

Finally, some algorithms may be more efficient with a smaller PAGESIZE than is normally used with disk paging.
