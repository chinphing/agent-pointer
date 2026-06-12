"""
快速排序 —— 面试加分版
====================
包含 4 个递进版本 + 性能对比测试
"""

import random
import time
import sys

# ─────────────────────────────────────────────
# 版本 1：经典快速排序（教科书版）
# ─────────────────────────────────────────────
def quicksort_classic(arr):
    """经典版：选最后一个元素为 pivot"""
    if len(arr) <= 1:
        return arr
    pivot = arr[-1]
    left  = [x for x in arr[:-1] if x <= pivot]
    right = [x for x in arr[:-1] if x > pivot]
    return quicksort_classic(left) + [pivot] + quicksort_classic(right)


# ─────────────────────────────────────────────
# 版本 2：随机 pivot + 原地分区（工程常用）
# ─────────────────────────────────────────────
def quicksort_random(arr, low=0, high=None):
    """随机选 pivot + 原地交换，避免已排序数组退化"""
    if high is None:
        high = len(arr) - 1
    if low < high:
        # 随机选 pivot，交换到末尾
        rand_idx = random.randint(low, high)
        arr[rand_idx], arr[high] = arr[high], arr[rand_idx]
        p = _partition(arr, low, high)
        quicksort_random(arr, low, p - 1)
        quicksort_random(arr, p + 1, high)
    return arr

def _partition(arr, low, high):
    pivot = arr[high]
    i = low - 1
    for j in range(low, high):
        if arr[j] <= pivot:
            i += 1
            arr[i], arr[j] = arr[j], arr[i]
    arr[i + 1], arr[high] = arr[high], arr[i + 1]
    return i + 1


# ─────────────────────────────────────────────
# 版本 3：三路切分（Dutch National Flag）
# ─────────────────────────────────────────────
def quicksort_3way(arr, low=0, high=None):
    """
    三路快排：arr[low:lt] < pivot
              arr[lt:gt] == pivot
              arr[gt:high+1] > pivot
    大量重复元素时秒杀普通快排
    """
    if high is None:
        high = len(arr) - 1
    if low >= high:
        return arr

    # 随机 pivot
    rand_idx = random.randint(low, high)
    arr[rand_idx], arr[high] = arr[high], arr[rand_idx]
    pivot = arr[high]

    lt, i, gt = low, low, high
    while i <= gt:
        if arr[i] < pivot:
            arr[lt], arr[i] = arr[i], arr[lt]
            lt += 1
            i += 1
        elif arr[i] > pivot:
            arr[i], arr[gt] = arr[gt], arr[i]
            gt -= 1
        else:
            i += 1

    quicksort_3way(arr, low, lt - 1)
    quicksort_3way(arr, gt + 1, high)
    return arr


# ─────────────────────────────────────────────
# 版本 4：尾递归优化（防栈溢出）
# ─────────────────────────────────────────────
def quicksort_tail(arr, low=0, high=None):
    """
    尾递归优化：每层递归只处理短的一侧，
    长的一侧用迭代继续，递归深度 ≤ O(log n)
    """
    if high is None:
        high = len(arr) - 1

    while low < high:
        rand_idx = random.randint(low, high)
        arr[rand_idx], arr[high] = arr[high], arr[rand_idx]
        p = _partition(arr, low, high)

        if p - low < high - p:
            # 左侧短 → 递归处理左侧
            quicksort_tail(arr, low, p - 1)
            low = p + 1          # 右侧用迭代
        else:
            # 右侧短 → 递归处理右侧
            quicksort_tail(arr, p + 1, high)
            high = p - 1         # 左侧用迭代
    return arr


# ─────────────────────────────────────────────
# 测试辅助
# ─────────────────────────────────────────────
def _test_sort(name, sort_fn, arr, expected=None):
    a = arr[:]
    t0 = time.perf_counter()
    try:
        result = sort_fn(a)
        t = time.perf_counter() - t0
        ok = (result == expected) if expected else (result == sorted(arr))
        tag = "✅" if ok else "❌"
        print(f"  {tag} {name}: {t*1000:.1f} ms")
        return ok, t
    except RecursionError:
        print(f"  💥 {name}: 递归溢出！")
        return False, float("inf")


def run_benchmark(size=10_000):
    """运行一组对比测试"""
    print(f"\n{'='*50}")
    print(f"📊 数据规模: n = {size:,}")
    print(f"{'='*50}")

    # --- 测试 1：随机数据 ---
    data = [random.randint(0, size) for _ in range(size)]
    expected = sorted(data)

    print(f"\n▶ 随机数据")
    t1, t2, t3, t4 = None, None, None, None
    _, t1 = _test_sort("经典版",        lambda a: quicksort_classic(a), data, expected)
    _, t2 = _test_sort("随机 pivot",    lambda a: quicksort_random(a),  data, expected)
    _, t3 = _test_sort("三路切分",      lambda a: quicksort_3way(a),   data, expected)
    _, t4 = _test_sort("尾递归优化",    lambda a: quicksort_tail(a),   data, expected)

    # --- 测试 2：已排序数组（经典版最差情况） ---
    data_sorted = list(range(size))
    expected2 = data_sorted[:]
    print(f"\n▶ 已排序数组（经典版最差情况）")
    _test_sort("经典版",        lambda a: quicksort_classic(a), data_sorted, expected2)
    _test_sort("随机 pivot",    lambda a: quicksort_random(a),  data_sorted, expected2)
    _test_sort("三路切分",      lambda a: quicksort_3way(a),   data_sorted, expected2)
    _test_sort("尾递归优化",    lambda a: quicksort_tail(a),   data_sorted, expected2)

    # --- 测试 3：大量重复元素 ---
    data_dup = [random.randint(0, size // 100) for _ in range(size)]
    expected3 = sorted(data_dup)
    print(f"\n▶ 大量重复元素（值域 n/100）")
    _test_sort("经典版",        lambda a: quicksort_classic(a), data_dup, expected3)
    _test_sort("随机 pivot",    lambda a: quicksort_random(a),  data_dup, expected3)
    _test_sort("三路切分",      lambda a: quicksort_3way(a),   data_dup, expected3)
    _test_sort("尾递归优化",    lambda a: quicksort_tail(a),   data_dup, expected3)


if __name__ == "__main__":
    # 调大递归限制（演示尾递归版本优势）
    sys.setrecursionlimit(1000000)

    n = int(sys.argv[1]) if len(sys.argv) > 1 else 10_000
    run_benchmark(n)
