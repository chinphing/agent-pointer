"""
快速排序 —— 面试加分版
========================
特性:
  1. 经典 Lomuto 分区
  2. 随机 pivot（避免最坏 O(n²)）
  3. 三路切分（Dutch National Flag，处理大量重复元素）
  4. 尾递归优化（限制递归深度）
  5. 性能对比 + 正确性验证
"""

import random
import sys
import time
from typing import List, Optional

# 大数据量排序时提升递归深度上限
sys.setrecursionlimit(1_000_000)


# ═══════════════════════════════════════════
# 版本一：经典快速排序（Lomuto 分区）
# ═══════════════════════════════════════════

def quicksort_classic(arr: List[int], low: int = 0, high: Optional[int] = None) -> None:
    """经典快速排序 —— 固定选最后一个元素为 pivot"""
    if high is None:
        high = len(arr) - 1
    if low < high:
        p = _partition_lomuto(arr, low, high)
        quicksort_classic(arr, low, p - 1)
        quicksort_classic(arr, p + 1, high)


def _partition_lomuto(arr: List[int], low: int, high: int) -> int:
    """Lomuto 分区方案：选 arr[high] 为 pivot"""
    pivot = arr[high]
    i = low - 1
    for j in range(low, high):
        if arr[j] <= pivot:
            i += 1
            arr[i], arr[j] = arr[j], arr[i]
    arr[i + 1], arr[high] = arr[high], arr[i + 1]
    return i + 1


# ═══════════════════════════════════════════
# 版本二：随机 pivot（⭐ 面试必问优化）
# ═══════════════════════════════════════════

def quicksort_random(arr: List[int], low: int = 0, high: Optional[int] = None) -> None:
    """
    随机化快速排序
    —— 将随机元素与末尾交换，再走 Lomuto，
       使最坏情况概率趋近于 0，期望 O(n log n)
    """
    if high is None:
        high = len(arr) - 1
    if low < high:
        # ⭐ 随机选 pivot，避免已排序数组退化到 O(n²)
        rand_idx = random.randint(low, high)
        arr[rand_idx], arr[high] = arr[high], arr[rand_idx]

        p = _partition_lomuto(arr, low, high)
        quicksort_random(arr, low, p - 1)
        quicksort_random(arr, p + 1, high)


# ═══════════════════════════════════════════
# 版本三：三路切分（⭐ 处理大量重复元素）
# ═══════════════════════════════════════════

def quicksort_three_way(arr: List[int], low: int = 0, high: Optional[int] = None) -> None:
    """
    三路切分快速排序（Dutch National Flag 算法）
    —— 将数组分为 <pivot、=pivot、>pivot 三个区间
    ⭐ 对包含大量重复元素的数组性能极佳
    """
    if high is None:
        high = len(arr) - 1
    if low >= high:
        return

    # 随机 pivot
    rand_idx = random.randint(low, high)
    arr[rand_idx], arr[high] = arr[high], arr[rand_idx]
    pivot = arr[high]

    # 三路切分
    lt, gt = low, high  # lt: 下个 < pivot 的位置；gt: 上个 > pivot 的位置
    i = low
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

    # 递归排序 < pivot 和 > pivot 区间（= pivot 的已就位）
    quicksort_three_way(arr, low, lt - 1)
    quicksort_three_way(arr, gt + 1, high)


# ═══════════════════════════════════════════
# 版本四：尾递归优化（防止栈溢出）
# ═══════════════════════════════════════════

def quicksort_tail(arr: List[int], low: int = 0, high: Optional[int] = None) -> None:
    """
    尾递归优化版
    —— 每次只对较短的子数组递归，较长的用循环
    ⭐ 将递归深度限制在 O(log n)
    """
    if high is None:
        high = len(arr) - 1

    while low < high:
        rand_idx = random.randint(low, high)
        arr[rand_idx], arr[high] = arr[high], arr[rand_idx]
        p = _partition_lomuto(arr, low, high)

        # 递归处理较短的子区间，循环处理较长的
        if p - low < high - p:
            quicksort_tail(arr, low, p - 1)
            low = p + 1
        else:
            quicksort_tail(arr, p + 1, high)
            high = p - 1


# ═══════════════════════════════════════════
# 辅助：验证排序是否正确
# ═══════════════════════════════════════════

def is_sorted(arr: List[int]) -> bool:
    return all(arr[i] <= arr[i + 1] for i in range(len(arr) - 1))


def assert_sorted(arr: List[int], name: str) -> None:
    assert is_sorted(arr), f"{name} 排序结果不正确！"
    print(f"  ✅ {name} 通过验证")


# ═══════════════════════════════════════════
# 测试 + 性能对比
# ═══════════════════════════════════════════

def run_tests() -> None:
    """功能性验证"""
    print("=" * 50)
    print("一、功能测试")
    print("=" * 50)

    test_cases = [
        ([], "空数组"),
        ([1], "单元素"),
        ([3, 1, 2], "普通数组"),
        ([5, 5, 5, 5], "全部相等"),
        ([3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5], "有重复元素"),
        ([1, 2, 3, 4, 5], "已升序"),
        ([9, 8, 7, 6, 5], "已降序"),
    ]

    for raw, desc in test_cases:
        print(f"\n  [{desc}] {raw}")
        for sort_fn, fn_name in [
            (quicksort_classic, "classic"),
            (quicksort_random, "random"),
            (quicksort_three_way, "three_way"),
            (quicksort_tail, "tail_recursive"),
        ]:
            arr = raw.copy()
            sort_fn(arr)
            assert_sorted(arr, fn_name)


def benchmark() -> None:
    """性能基准"""
    print("\n" + "=" * 50)
    print("二、性能对比")
    print("=" * 50)

    sizes = [1_000, 10_000, 100_000]
    scenarios = {
        "随机数据": lambda n: [random.randint(0, 1_000_000) for _ in range(n)],
        "大量重复": lambda n: [random.randint(0, 10) for _ in range(n)],
        "已排序": lambda n: list(range(n)),
    }

    algorithms = [
        (quicksort_classic, "经典（固定 pivot）"),
        (quicksort_random, "随机 pivot"),
        (quicksort_three_way, "三路切分"),
        (quicksort_tail, "尾递归优化"),
    ]

    Python_sorted = sorted  # 内置排序作为基准

    for scenario_name, gen_fn in scenarios.items():
        print(f"\n  📊 {scenario_name}:")
        for size in sizes:
            data = gen_fn(size)
            # 用内置 sorted 做参照
            if size <= 10_000:
                t0 = time.perf_counter()
                Python_sorted(data.copy())
                t_builtin = time.perf_counter() - t0
            else:
                t_builtin = None

            for sort_fn, fn_name in algorithms:
                # 经典版本对已排序数组退化为 O(n²)，跳过
                if sort_fn is quicksort_classic and scenario_name == "已排序" and size > 1_000:
                    print(f"    {fn_name:18s} | n={size:<7} | ⚠️  跳过（会退化 O(n²)）")
                    continue
                # 经典版本在大规模随机数据可能递归过深，跳过
                if sort_fn is quicksort_classic and size >= 100_000:
                    print(f"    {fn_name:18s} | n={size:<7} | ⚠️  跳过（可能递归超限）")
                    continue

                arr = data.copy()
                t0 = time.perf_counter()
                sort_fn(arr)
                elapsed = time.perf_counter() - t0

                if not is_sorted(arr):
                    print(f"    {fn_name:18s} | n={size:<7} | ❌ 排序错误！")
                    continue

                speedup = ""
                if t_builtin and t_builtin > 0:
                    ratio = elapsed / t_builtin
                    speedup = f" | Python sorted 的 {ratio:.2f} 倍"

                print(f"    {fn_name:18s} | n={size:<7} | {elapsed:.6f}s{speedup}")


# ═══════════════════════════════════════════
# 主入口
# ═══════════════════════════════════════════

if __name__ == "__main__":
    run_tests()
    benchmark()

    print("\n" + "=" * 50)
    print("使用示例")
    print("=" * 50)
    data = [3, 6, 8, 10, 1, 2, 1]
    print(f"  原始: {data}")
    quicksort_three_way(data)
    print(f"  排序后: {data}")
